//! SPEC-0300-0011 §7 (R-50~R-58): OS-trash delete and permanent delete.
//! Planned API (not yet implemented — see `super::mod` doc comment):
//!
//! ```ignore
//! pub fn delete_to_trash(paths: &[PathBuf]) -> OpSummary;   // super::transfer::OpSummary
//! pub fn delete_permanently(paths: &[PathBuf]) -> OpSummary;
//! ```
//!
//! R-50-1 explicitly defers the concrete mechanism (which crate, if any) to
//! architecture-mapper/code-generator; these tests only assert the
//! observable contract the spec requires:
//! - the original path is gone after a successful delete (R-50/R-56)
//! - no application-owned staging directory is created (R-50, R-57, R-90 —
//!   the `undo_trash` mechanism `main` used must not reappear)
//! - batch semantics (ok/failed/total/failures) match `super::transfer::OpSummary` (R-51, R-56-3)
//! - failures in one item do not stop the rest of the batch (R-80)
//!
//! **Not covered here (explicitly out of unit-test scope):** actually
//! verifying that a deleted item is restorable from the real OS Recycle Bin
//! (AC-37) is an OS-level integration/manual-QA concern. Automated tests
//! deliberately avoid writing into the developer's real Recycle Bin on
//! every run; combined with R-50-1 deferring the crate choice, this
//! verification is left to integration/manual QA once code-generator picks
//! a concrete mechanism.

use std::fs;
use std::path::{Path, PathBuf};

use super::transfer::{Failure, OpSummary};

/// R-50/R-56: which delete implementation `run_delete_batch` performs. A
/// plain enum, not a generic backend parameter (SPEC-0400-0001 §4.1: no
/// trait, no generics/dyn dispatch over the filesystem) — it only selects
/// between the two concrete operations below.
#[derive(Debug, Clone, Copy)]
enum DeleteMode {
    /// R-50/R-50-1: OS-standard trash bin (Windows Recycle Bin), via the
    /// `trash` crate. No application-owned staging directory (R-57, R-90).
    Trash,
    /// R-56: bypasses the trash bin entirely (Shift-modified delete).
    Permanent,
}

fn run_delete_batch(mode: DeleteMode, paths: &[PathBuf]) -> OpSummary {
    let mut summary = OpSummary {
        total: paths.len() as u32,
        ..Default::default()
    };

    for path in paths {
        if !path.exists() {
            summary.failed += 1;
            summary.failures.push(Failure {
                path: path.clone(),
                code: "not_found".to_string(),
                message: "source not found".to_string(),
            });
            continue;
        }

        let result = delete_one(mode, path);
        match result {
            Ok(()) => summary.ok += 1,
            Err(message) => {
                summary.failed += 1;
                summary.failures.push(Failure {
                    path: path.clone(),
                    code: "io_error".to_string(),
                    message,
                });
            }
        }
    }

    summary
}

fn delete_one(mode: DeleteMode, path: &Path) -> Result<(), String> {
    match mode {
        DeleteMode::Trash => trash::delete(path).map_err(|e| e.to_string()),
        DeleteMode::Permanent => {
            if path.is_dir() {
                fs::remove_dir_all(path).map_err(|e| e.to_string())
            } else {
                fs::remove_file(path).map_err(|e| e.to_string())
            }
        }
    }
}

/// R-50/R-51: move a batch of paths to the OS-standard trash bin.
pub fn delete_to_trash(paths: &[PathBuf]) -> OpSummary {
    run_delete_batch(DeleteMode::Trash, paths)
}

/// R-56/R-56-3: permanently delete a batch of paths (bypassing the trash
/// bin entirely).
pub fn delete_permanently(paths: &[PathBuf]) -> OpSummary {
    run_delete_batch(DeleteMode::Permanent, paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // SPEC-0300-0011 AC-37 (partial, unit-level slice — see module doc
    // comment for what is deliberately out of scope): after a successful
    // trash-delete, the item is gone from its original location.
    #[test]
    fn deleted_item_is_gone_from_its_original_location() {
        let dir = tempdir();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"1").expect("write target");

        let summary = delete_to_trash(&[target.clone()]);

        assert_eq!(summary.ok, 1);
        assert!(!target.exists());
    }

    // SPEC-0300-0011 AC-38 (R-50, R-57, R-90): no application-owned staging
    // directory (the `main`-style `undo_trash` mechanism) is created as a
    // side effect of deleting.
    #[test]
    fn ac38_no_application_staging_directory_is_created() {
        let dir = tempdir();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"1").expect("write target");

        let _ = delete_to_trash(&[target]);

        assert!(
            !dir.path().join("undo_trash").exists(),
            "R-50/R-57/R-90: no independent staging folder may be created"
        );
    }

    // SPEC-0300-0011 AC-44 (R-51): multiple selected items are deleted in
    // one batch call, and the result exposes ok/failed/total/failures.
    #[test]
    fn ac44_batch_deletes_multiple_items_and_reports_counts() {
        let dir = tempdir();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"1").expect("write a");
        fs::write(&b, b"2").expect("write b");

        let summary = delete_to_trash(&[a.clone(), b.clone()]);

        assert_eq!(summary.total, 2);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 0);
        assert!(!a.exists());
        assert!(!b.exists());
    }

    // SPEC-0300-0011 AC-45 (R-51, R-80): a failure on one item (a missing
    // path, standing in for any single-item failure) does not prevent the
    // rest of the batch from completing, and the failure is reported.
    #[test]
    fn ac45_partial_failure_does_not_prevent_the_rest_of_the_batch() {
        let dir = tempdir();
        let missing = dir.path().join("missing.txt");
        let existing = dir.path().join("existing.txt");
        fs::write(&existing, b"1").expect("write existing");

        let summary = delete_to_trash(&[missing, existing.clone()]);

        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 1);
        assert!(!existing.exists());
    }

    // SPEC-0300-0011 AC-46 (R-56): permanent delete removes the item from
    // its original location, exactly like trash-delete does, but via the
    // dedicated permanent-delete entry point (the "does not land in the OS
    // Recycle Bin" half of AC-46 is, per this module's doc comment, an
    // OS-level integration concern beyond this unit test).
    #[test]
    fn ac46_permanent_delete_removes_the_item() {
        let dir = tempdir();
        let target = dir.path().join("a.txt");
        fs::write(&target, b"1").expect("write target");

        let summary = delete_permanently(&[target.clone()]);

        assert_eq!(summary.ok, 1);
        assert!(!target.exists());
    }

    // SPEC-0300-0011 AC-46-2 (R-56-3, R-80): permanent delete is also a
    // batch operation with the same ok/failed/total/failures contract, and
    // partial failure still lets the rest of the batch complete.
    #[test]
    fn ac46_2_permanent_delete_batch_reports_counts_and_continues_on_partial_failure() {
        let dir = tempdir();
        let missing = dir.path().join("missing.txt");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, b"1").expect("write a");
        fs::write(&b, b"2").expect("write b");

        let summary = delete_permanently(&[missing, a.clone(), b.clone()]);

        assert_eq!(summary.total, 3);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 1);
        assert!(!a.exists());
        assert!(!b.exists());
    }
}
