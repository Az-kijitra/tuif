//! SPEC-0300-0011 §8 (R-60~R-64): the rename command. Planned API (not yet
//! implemented — see `super::mod` doc comment):
//!
//! ```ignore
//! pub fn rename_entry(path: &Path, new_name: &str)
//!     -> Result<PathBuf, super::preflight::MutatePreflightError>;
//! ```
//!
//! R-64: this command takes `(path, new_name)` and returns the new full path (SHOULD-80) or the failure
//! (not a batch result — renaming is always a single-entry operation,
//! R-12).

use std::fs;
use std::path::{Path, PathBuf};

use super::preflight::{self, MutatePreflightError};

/// R-60~R-64: rename `path` to `new_name` within its parent directory.
/// Delegates all validation (empty/forbidden-character/already_exists) to
/// `preflight::preflight_rename` (AC-66: shared identically with create) —
/// on any rejection, the original entry is left completely untouched.
pub fn rename_entry(path: &Path, new_name: &str) -> Result<PathBuf, MutatePreflightError> {
    preflight::preflight_rename(path, new_name)?;

    let parent = path.parent().ok_or(MutatePreflightError::ParentNotFound)?;
    let target = parent.join(new_name.trim());
    fs::rename(path, &target).map_err(preflight::map_io_error)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // SPEC-0300-0011 AC-48 (R-60, R-61): renaming an entry with a valid new
    // name succeeds and the entry is found at the new name (and no longer
    // at the old one).
    #[test]
    fn ac48_rename_succeeds_and_moves_the_entry_to_the_new_name() {
        let dir = tempdir();
        let original = dir.path().join("old.txt");
        fs::write(&original, b"1").expect("write original");

        let result = rename_entry(&original, "new.txt");

        assert!(result.is_ok());
        assert!(!original.exists());
        assert!(dir.path().join("new.txt").exists());
    }

    // SPEC-0300-0011 AC-55 (R-36, R-63): renaming onto an existing sibling
    // name fails with `already_exists`, and the original entry is left
    // completely untouched (so the dialog can let the user retry).
    #[test]
    fn ac55_rename_onto_existing_name_fails_and_leaves_original_untouched() {
        let dir = tempdir();
        let original = dir.path().join("old.txt");
        let existing_sibling = dir.path().join("existing.txt");
        fs::write(&original, b"1").expect("write original");
        fs::write(&existing_sibling, b"2").expect("write sibling");

        let result = rename_entry(&original, "existing.txt");

        assert!(result.is_err());
        assert!(
            original.exists(),
            "the original entry must remain in place after a rejected rename"
        );
        assert_eq!(fs::read(&existing_sibling).expect("read sibling"), b"2");
    }

    // SPEC-0300-0011 AC-56 (R-36): rename never falls back to an
    // auto-generated alternate name on conflict (unlike copy/move's
    // confirm-dialog flow) — repeatedly attempting the same colliding name
    // always fails the same way, never silently succeeds under a different
    // name.
    #[test]
    fn ac56_rename_never_auto_renames_on_conflict() {
        let dir = tempdir();
        let original = dir.path().join("old.txt");
        let existing_sibling = dir.path().join("existing.txt");
        fs::write(&original, b"1").expect("write original");
        fs::write(&existing_sibling, b"2").expect("write sibling");

        let first_attempt = rename_entry(&original, "existing.txt");
        let second_attempt = rename_entry(&original, "existing.txt");

        assert!(first_attempt.is_err());
        assert!(second_attempt.is_err());
        // No "existing (2).txt"-style auto-renamed sibling must appear.
        assert!(!dir.path().join("existing (2).txt").exists());
    }

    // SPEC-0300-0011 R-62 (shared validation): renaming to a forbidden name
    // is rejected exactly like preflight_rename would reject it (this
    // command is expected to delegate to the same validation).
    #[test]
    fn rename_with_a_forbidden_character_is_rejected() {
        let dir = tempdir();
        let original = dir.path().join("old.txt");
        fs::write(&original, b"1").expect("write original");

        let result = rename_entry(&original, "bad|name.txt");

        assert!(result.is_err());
        assert!(original.exists());
    }
}
