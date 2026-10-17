//! SPEC-0300-0011 §9 (R-70~R-78): the create command. Planned API (not yet
//! implemented — see `super::mod` doc comment):
//!
//! ```ignore
//! pub fn create_entry(parent: &Path, name: &str, kind: &str)
//!     -> Result<(), super::preflight::MutatePreflightError>;
//! ```
//!
//! R-78: `(parent, name, kind)` in, success/failure only. R-73: `kind` is
//! `"file"` (creates a 0-byte file) or `"folder"` (creates a directory);
//! anything else is `invalid_kind` (covered in `preflight.rs`).

use std::fs;
use std::path::Path;

use super::preflight::{self, MutatePreflightError};

/// R-70~R-78: create a new entry named `name` under `parent`. Delegates all
/// validation (kind/empty/forbidden-character/already_exists/parent
/// existence) to `preflight::preflight_create` (AC-66: shared identically
/// with rename) — on any rejection, nothing is created or overwritten.
pub fn create_entry(parent: &Path, name: &str, kind: &str) -> Result<(), MutatePreflightError> {
    preflight::preflight_create(parent, name, kind)?;

    let target = parent.join(name.trim());
    match kind {
        "folder" => fs::create_dir(&target).map_err(preflight::map_io_error),
        _ => create_new_file(&target),
    }
}

/// Creates an empty file only if nothing exists at `target` (`create_new`), so
/// an entry that appeared after preflight is never truncated (TOCTOU-safe).
pub(crate) fn create_new_file(target: &Path) -> Result<(), MutatePreflightError> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map(|_| ())
        .map_err(preflight::map_io_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // SPEC-0300-0011 AC-57 (R-70, R-73): creating with kind "file" creates
    // a 0-byte file at the given name in the parent directory.
    #[test]
    fn ac57_create_file_creates_a_zero_byte_file() {
        let dir = tempdir();

        let result = create_entry(dir.path(), "new.txt", "file");

        assert!(result.is_ok());
        let created = dir.path().join("new.txt");
        assert!(created.is_file());
        assert_eq!(fs::metadata(&created).expect("metadata").len(), 0);
    }

    // SPEC-0300-0011 AC-58 (R-70, R-73): creating with kind "folder"
    // creates a directory.
    #[test]
    fn ac58_create_folder_creates_a_directory() {
        let dir = tempdir();

        let result = create_entry(dir.path(), "New Folder", "folder");

        assert!(result.is_ok());
        assert!(dir.path().join("New Folder").is_dir());
    }

    // SPEC-0300-0011 AC-63 (R-74): creating with a name that already
    // exists in the parent is rejected as `already_exists`, and nothing is
    // overwritten.
    #[test]
    fn ac63_create_with_existing_name_fails_without_overwriting() {
        let dir = tempdir();
        let existing = dir.path().join("a.txt");
        fs::write(&existing, b"original").expect("write existing");

        let result = create_entry(dir.path(), "a.txt", "file");

        assert!(result.is_err());
        assert_eq!(fs::read(&existing).expect("read existing"), b"original");
    }

    // SPEC-0300-0011 R-62 (shared validation, AC-66): creating with a
    // forbidden-character name is rejected exactly like preflight_create
    // would reject it (this command is expected to delegate to the same
    // validation as rename).
    #[test]
    fn create_with_a_forbidden_character_is_rejected() {
        let dir = tempdir();

        let result = create_entry(dir.path(), "bad<name>.txt", "file");

        assert!(result.is_err());
        assert!(!dir.path().join("bad<name>.txt").exists());
    }
}
