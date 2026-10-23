//! SPEC-0300-0011 §4/§8/§9 (R-24, R-36, R-62, R-73~R-75): preflight checks
//! run before a transfer (copy/move) batch, a rename, or a create. Planned
//! API (not yet implemented — see `super::mod` doc comment):
//!
//! ```ignore
//! pub enum TransferPreflightError {
//!     NoItems, SamePath, DestinationInsideSource,
//!     DestinationNotFound, DestinationNotDir, InvalidPath, NotFound,
//! }
//! pub fn preflight_transfer(items: &[PathBuf], destination: &Path)
//!     -> Result<(), TransferPreflightError>;
//!
//! pub enum MutatePreflightError {
//!     Required, InvalidName, AlreadyExists, ParentNotFound, InvalidKind,
//! }
//! pub fn preflight_rename(path: &Path, new_name: &str) -> Result<(), MutatePreflightError>;
//! pub fn preflight_create(parent: &Path, name: &str, kind: &str) -> Result<(), MutatePreflightError>;
//! ```
//!
//! `preflight_rename`/`preflight_create` are expected to delegate their name
//! checks to `super::name_validation::validate_name` (AC-66: rename and
//! create share identical name validation).

use std::path::{Path, PathBuf};

use super::name_validation::{self, NameError};
use crate::shared::identity::{is_strictly_within, same_entity};

/// R-24: preflight rejection codes for a copy/move batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPreflightError {
    NoItems,
    SamePath,
    DestinationInsideSource,
    DestinationNotFound,
    DestinationNotDir,
    InvalidPath,
    NotFound,
}

/// R-24: at least these conditions must be detected and the batch rejected
/// before any transfer work is attempted (AC-72: zero side effects).
pub fn preflight_transfer(
    items: &[PathBuf],
    destination: &Path,
) -> Result<(), TransferPreflightError> {
    if items.is_empty() {
        return Err(TransferPreflightError::NoItems);
    }

    if destination.to_str().is_none() {
        return Err(TransferPreflightError::InvalidPath);
    }
    for item in items {
        if item.to_str().is_none() {
            return Err(TransferPreflightError::InvalidPath);
        }
    }

    if !destination.exists() {
        return Err(TransferPreflightError::DestinationNotFound);
    }
    if !destination.is_dir() {
        return Err(TransferPreflightError::DestinationNotDir);
    }

    // code-reviewer (2026-09-23, BLK-1): these are two separate passes, not
    // one. `SamePath`/`DestinationInsideSource` are structural hazards that
    // must be detected wherever they occur in the batch — attempting the
    // transfer at all would be unsafe (e.g. the self-referential recursive
    // copy that used to overflow the stack before this function was wired
    // in). `NotFound` is an ordinary per-item failure that the caller
    // (`run_transfer_batch`) deliberately excludes from whole-batch
    // rejection so that one missing source does not block the rest of the
    // batch (AC-68). A single combined loop that `return`s on the first
    // match would let a `NotFound` item earlier in the list hide a
    // structurally dangerous item later in the same list — exactly the gap
    // BLK-1 found (a batch of `[missing_item, self_referential_item]`
    // reported only `NotFound` and let the dangerous item through
    // untouched). Scanning all items for the structural hazards first closes
    // that gap regardless of item order.
    //
    // MUST-59 / R-37-1 (SPEC-0200-0001 §3.3.13-2): both structural checks use
    // the canonical identity (`identity`), not string comparison, so letter
    // case, junctions/symlinks and `\\?\` spellings cannot defeat them. Only
    // when identity cannot be determined at all does it fall back to the
    // string comparison (the pre-existing behavior).
    for item in items {
        let same = same_entity(item, destination).unwrap_or(item == destination);
        if same {
            return Err(TransferPreflightError::SamePath);
        }
        if item.is_dir() {
            let inside = is_strictly_within(item, destination)
                .unwrap_or_else(|| destination.starts_with(item));
            if inside {
                return Err(TransferPreflightError::DestinationInsideSource);
            }
        }
    }

    for item in items {
        if !item.exists() {
            return Err(TransferPreflightError::NotFound);
        }
    }

    Ok(())
}

/// R-62/R-73~R-75: preflight rejection codes shared by rename (§8) and
/// create (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutatePreflightError {
    Required,
    InvalidName,
    AlreadyExists,
    ParentNotFound,
    InvalidKind,
}

impl From<NameError> for MutatePreflightError {
    fn from(err: NameError) -> Self {
        match err {
            NameError::Required => MutatePreflightError::Required,
            NameError::InvalidName => MutatePreflightError::InvalidName,
        }
    }
}

/// Maps an unexpected I/O failure (occurring *after* preflight already
/// passed, e.g. a concurrent filesystem change) onto the closest
/// `MutatePreflightError` variant. This is a pragmatic fallback for
/// `rename::rename_entry`/`create::create_entry`, not a new error
/// taxonomy — the planned API for this module only defines the 5 variants
/// above.
pub(crate) fn map_io_error(err: std::io::Error) -> MutatePreflightError {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => MutatePreflightError::AlreadyExists,
        std::io::ErrorKind::NotFound => MutatePreflightError::ParentNotFound,
        _ => MutatePreflightError::InvalidName,
    }
}

/// R-36/R-62/R-63: renaming `path` to `new_name` is rejected up front when
/// the new name is invalid or a sibling with that name already exists.
pub fn preflight_rename(path: &Path, new_name: &str) -> Result<(), MutatePreflightError> {
    name_validation::validate_name(new_name)?;

    let parent = path.parent().ok_or(MutatePreflightError::ParentNotFound)?;
    let candidate = parent.join(new_name.trim());
    if candidate.exists() {
        // A case-only rename (`a.txt` -> `A.txt`) resolves to the entry itself
        // on case-insensitive filesystems: that is not a conflict.
        let is_self = candidate.file_name() != path.file_name()
            && same_entity(path, &candidate) == Some(true);
        if !is_self {
            return Err(MutatePreflightError::AlreadyExists);
        }
    }

    Ok(())
}

/// R-36/R-62/R-73~R-75: creating `name` (of `kind`) under `parent` is
/// rejected up front for an invalid kind, invalid name, missing parent, or
/// an existing sibling with that name.
pub fn preflight_create(parent: &Path, name: &str, kind: &str) -> Result<(), MutatePreflightError> {
    if kind != "file" && kind != "folder" {
        return Err(MutatePreflightError::InvalidKind);
    }

    name_validation::validate_name(name)?;

    if !parent.exists() {
        return Err(MutatePreflightError::ParentNotFound);
    }

    let candidate = parent.join(name.trim());
    if candidate.exists() {
        return Err(MutatePreflightError::AlreadyExists);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // SPEC-0300-0011 AC-17 (R-24): an empty item list is rejected as
    // `no_items` before any I/O is attempted.
    #[test]
    fn ac17_empty_items_is_no_items_error() {
        let dir = tempdir();
        let result = preflight_transfer(&[], dir.path());
        assert_eq!(result, Err(TransferPreflightError::NoItems));
    }

    // SPEC-0300-0011 AC-16 (R-24): a destination that does not exist on
    // disk is rejected as `destination_not_found`.
    #[test]
    fn ac16_missing_destination_is_destination_not_found() {
        let dir = tempdir();
        let source = dir.path().join("a.txt");
        fs::write(&source, b"1").expect("write source");
        let missing_destination = dir.path().join("does-not-exist");

        let result = preflight_transfer(&[source], &missing_destination);
        assert_eq!(result, Err(TransferPreflightError::DestinationNotFound));
    }

    // SPEC-0300-0011 AC-15 (R-24): moving/copying a directory into its own
    // subtree is rejected as `destination_inside_source`.
    #[test]
    fn ac15_destination_inside_source_is_rejected() {
        let dir = tempdir();
        let source_dir = dir.path().join("parent");
        let nested_destination = source_dir.join("child");
        fs::create_dir_all(&nested_destination).expect("create nested dirs");

        let result = preflight_transfer(&[source_dir], &nested_destination);
        assert_eq!(result, Err(TransferPreflightError::DestinationInsideSource));
    }

    // A destination that exists but is a regular file (not a directory)
    // must be rejected as `destination_not_dir`.
    #[test]
    fn destination_that_is_a_file_is_destination_not_dir() {
        let dir = tempdir();
        let source = dir.path().join("a.txt");
        fs::write(&source, b"1").expect("write source");
        let destination_file = dir.path().join("not-a-dir.txt");
        fs::write(&destination_file, b"1").expect("write destination file");

        let result = preflight_transfer(&[source], &destination_file);
        assert_eq!(result, Err(TransferPreflightError::DestinationNotDir));
    }

    // A source item that does not exist on disk is rejected as `not_found`.
    #[test]
    fn missing_source_item_is_not_found() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let missing_source = dir.path().join("missing.txt");

        let result = preflight_transfer(&[missing_source], &destination);
        assert_eq!(result, Err(TransferPreflightError::NotFound));
    }

    // SPEC-0300-0011 AC-72 (R-83): a preflight rejection must not perform
    // any part of the transfer — zero side effects on disk.
    #[test]
    fn ac72_rejected_preflight_has_zero_side_effects() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        // `no_items` rejection: destination directory must remain empty.
        let _ = preflight_transfer(&[], &destination);
        let entries: Vec<_> = fs::read_dir(&destination)
            .expect("read destination")
            .collect();
        assert!(
            entries.is_empty(),
            "no_items preflight rejection must not create anything"
        );
    }

    // SPEC-0300-0011 AC-55 (R-36, R-63): renaming onto an existing sibling
    // name is rejected as `already_exists` (no confirm dialog, no
    // auto-rename — R-36 applies to rename/create regardless of the
    // copy/move confirm-dialog behavior in R-30).
    #[test]
    fn ac55_rename_onto_existing_name_is_already_exists() {
        let dir = tempdir();
        let target = dir.path().join("a.txt");
        let existing_sibling = dir.path().join("b.txt");
        fs::write(&target, b"1").expect("write target");
        fs::write(&existing_sibling, b"2").expect("write sibling");

        let result = preflight_rename(&target, "b.txt");
        assert_eq!(result, Err(MutatePreflightError::AlreadyExists));
    }

    // SPEC-0300-0011 AC-63 (R-74): creating with a name that already exists
    // in the parent directory is rejected as `already_exists`.
    #[test]
    fn ac63_create_with_existing_name_is_already_exists() {
        let dir = tempdir();
        let existing = dir.path().join("a.txt");
        fs::write(&existing, b"1").expect("write existing");

        let result = preflight_create(dir.path(), "a.txt", "file");
        assert_eq!(result, Err(MutatePreflightError::AlreadyExists));
    }

    // SPEC-0300-0011 R-75: a parent directory that does not exist is
    // rejected as `parent_not_found`.
    #[test]
    fn create_with_missing_parent_is_parent_not_found() {
        let dir = tempdir();
        let missing_parent = dir.path().join("does-not-exist");

        let result = preflight_create(&missing_parent, "a.txt", "file");
        assert_eq!(result, Err(MutatePreflightError::ParentNotFound));
    }

    // SPEC-0300-0011 R-73: only "file"/"folder" are valid kinds; anything
    // else is rejected as `invalid_kind`.
    #[test]
    fn create_with_unrecognized_kind_is_invalid_kind() {
        let dir = tempdir();
        let result = preflight_create(dir.path(), "a.txt", "symlink");
        assert_eq!(result, Err(MutatePreflightError::InvalidKind));
    }

    // SPEC-0300-0011 AC-66 (R-62): rename and create reject the exact same
    // invalid names via the same underlying validation (name_validation).
    #[test]
    fn ac66_rename_and_create_share_identical_name_validation() {
        let dir = tempdir();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"1").expect("write target");

        let forbidden_name = "bad|name.txt";
        assert_eq!(
            preflight_rename(&target, forbidden_name),
            Err(MutatePreflightError::InvalidName)
        );
        assert_eq!(
            preflight_create(dir.path(), forbidden_name, "file"),
            Err(MutatePreflightError::InvalidName)
        );

        assert_eq!(
            preflight_rename(&target, ""),
            Err(MutatePreflightError::Required)
        );
        assert_eq!(
            preflight_create(dir.path(), "", "file"),
            Err(MutatePreflightError::Required)
        );
    }
}
