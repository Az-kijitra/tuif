//! TDD (unit-test-writer, 2026-09-27): `archive::win_fs` — the no-replace commit
//! (`move_no_replace`, MUST-66) and `free_space` (L-3).
//!
//! Specs: SPEC-0300-0014 R-326-3 / R-376 / R-370 (never replace without an
//! explicit instruction), R-365 L-3; SPEC-0200-0001 §3.4.13-2 (MUST-66),
//! §3.4.13-7 (MUST-72). The tests are Windows-oriented (`MoveFileExW` without
//! `MOVEFILE_REPLACE_EXISTING`); the non-Windows fallback (`hard_link` +
//! `remove_file`) satisfies the same observable contract, so they run everywhere.

use super::test_support::*;
use super::win_fs::*;
use std::io::ErrorKind;

// The success path: the file moves, the source name disappears.
#[test]
fn move_no_replace_moves_to_a_free_name() {
    let tmp = tempdir();
    let from = write_file(tmp.path(), "from.tmp", b"payload");
    let to = tmp.path().join("to.txt");
    move_no_replace(&from, &to).expect("move");
    assert_eq!(read_string(&to), "payload");
    assert!(!from.exists(), "the source must be gone after the move");
}

// MUST-66: an existing destination is NEVER replaced; the error is
// `AlreadyExists` (ERROR_ALREADY_EXISTS / ERROR_FILE_EXISTS) and both files
// are untouched.
#[test]
fn must66_move_no_replace_refuses_an_existing_destination() {
    let tmp = tempdir();
    let from = write_file(tmp.path(), "from.tmp", b"new content");
    let to = write_file(tmp.path(), "to.txt", b"precious original");
    let err = move_no_replace(&from, &to).expect_err("must not replace");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists, "{err:?}");
    assert_eq!(read_string(&to), "precious original", "the existing file must be unchanged");
    assert_eq!(read_string(&from), "new content", "the source must be left in place");
}

// Same for a read-only destination and for a destination that differs only in
// letter case (NTFS is case-insensitive: it is the same name).
#[cfg(windows)]
#[test]
fn must66_move_no_replace_treats_a_case_variant_as_existing_on_windows() {
    let tmp = tempdir();
    let from = write_file(tmp.path(), "from.tmp", b"new");
    write_file(tmp.path(), "Report.TXT", b"original");
    let err = move_no_replace(&from, &tmp.path().join("report.txt")).expect_err("must not replace");
    assert_eq!(err.kind(), ErrorKind::AlreadyExists, "{err:?}");
    assert_eq!(read_string(&tmp.path().join("Report.TXT")), "original");
}

// An existing directory of that name is not replaced either.
#[test]
fn must66_move_no_replace_refuses_an_existing_directory() {
    let tmp = tempdir();
    let from = write_file(tmp.path(), "from.tmp", b"new");
    let to = tmp.path().join("dir");
    std::fs::create_dir(&to).unwrap();
    write_file(&to, "inside.txt", b"inside");
    assert!(move_no_replace(&from, &to).is_err());
    assert_eq!(read_string(&to.join("inside.txt")), "inside");
    assert!(from.exists());
}

// R-367: a link (even with no target) on the destination name is not written through.
#[test]
fn must66_move_no_replace_does_not_write_through_a_dangling_link() {
    let tmp = tempdir();
    let from = write_file(tmp.path(), "from.tmp", b"new");
    let link = tmp.path().join("f.txt");
    let target = tmp.path().join("scratch");
    if !make_dangling_dir_link(&link, &target) {
        skip("cannot create a dangling junction/symlink here");
        return;
    }
    assert!(move_no_replace(&from, &link).is_err());
    assert!(!target.exists());
    assert!(from.exists());
    remove_dir_link(&link);
}

#[test]
fn move_no_replace_reports_a_missing_source_as_not_found() {
    let tmp = tempdir();
    let to = tmp.path().join("to.txt");
    let err = move_no_replace(&tmp.path().join("missing.tmp"), &to).expect_err("no source");
    assert_eq!(err.kind(), ErrorKind::NotFound, "{err:?}");
    assert!(!to.exists());
}

#[test]
fn move_no_replace_fails_when_the_destination_directory_is_missing() {
    let tmp = tempdir();
    let from = write_file(tmp.path(), "from.tmp", b"x");
    assert!(move_no_replace(&from, &tmp.path().join("no-dir").join("to.txt")).is_err());
    assert!(from.exists());
}

// L-3: the free-space query.
#[test]
fn free_space_of_an_existing_directory_is_positive() {
    let tmp = tempdir();
    let free = free_space(tmp.path()).expect("free_space");
    assert!(free > 0, "the temp volume has some free space");
}

#[test]
fn free_space_of_a_missing_path_is_an_error_not_a_bogus_number() {
    let tmp = tempdir();
    assert!(free_space(&tmp.path().join("no-such-dir")).is_err());
}
