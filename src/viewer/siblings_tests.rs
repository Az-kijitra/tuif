//! TDD (unit-test-writer, 2026-09-27): previous/next file list ordering
//! (name, case-insensitive), the R-491-3 ordinal tie-break, hidden-file
//! inclusion and directory exclusion.
//!
//! Specs: SPEC-0300-0015 R-491, R-491-1, R-491-2, R-491-3; AC-490, AC-490-1.
//!
//! NOTE for code-generator (testing limitation, documented in the handback
//! report): real symlinks/junctions (R-495, AC-453/AC-453-1) are not created
//! here because `std::os::windows::fs::symlink_file` normally needs an
//! elevated/Developer-Mode process; that behavior is instead pinned as a pure
//! function over already-resolved paths in `viewer/session_tests.rs`
//! (`session::is_allowed`).

use super::siblings::*;
use std::cmp::Ordering;
use std::fs;

fn touch(dir: &std::path::Path, name: &str) {
    fs::write(dir.join(name), b"x").unwrap();
}

// ---------------------------------------------------------------------
// compare_names (R-491, R-491-3)
// ---------------------------------------------------------------------

#[test]
fn compare_names_is_case_insensitive_for_clearly_different_names() {
    assert_eq!(compare_names("a.txt", "B.txt"), Ordering::Less);
    assert_eq!(compare_names("c.png", "B.txt"), Ordering::Greater);
}

#[test]
fn compare_names_r491_3_tie_break_is_the_original_ordinal_comparison() {
    // AC-490-1: case-insensitively equal names ("README.txt" / "Readme.txt")
    // are ordered by comparing the ORIGINAL strings: 'E' (U+0045) < 'e'
    // (U+0065), so "README.txt" sorts before "Readme.txt".
    assert_eq!(compare_names("README.txt", "Readme.txt"), Ordering::Less);
    assert_eq!(compare_names("Readme.txt", "README.txt"), Ordering::Greater);
}

#[test]
fn compare_names_identical_strings_are_equal() {
    assert_eq!(compare_names("a.txt", "a.txt"), Ordering::Equal);
}

// ---------------------------------------------------------------------
// build_sibling_list (R-491, R-491-1, R-491-2)
// ---------------------------------------------------------------------

fn names_of(paths: &[std::path::PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn ac490_files_are_ordered_by_name_hidden_files_included_directories_excluded() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "B.txt");
    touch(dir.path(), "a.txt");
    touch(dir.path(), "c.png");
    touch(dir.path(), ".hidden");
    fs::create_dir(dir.path().join("sub")).unwrap();

    let list = build_sibling_list(dir.path()).expect("build_sibling_list");
    let names = names_of(&list);

    assert_eq!(names, vec![".hidden", "a.txt", "B.txt", "c.png"]);
    assert!(!names.contains(&"sub".to_string()), "directories must be excluded (R-491)");
}

// Test fix (code-generator, 2026-09-27): the original fixture also created
// "Readme.txt" alongside "README.txt" in the same directory to exercise the
// R-491-3 ordinal tie-break end-to-end. That precondition is physically
// impossible to create via `std::fs::write` on this app's target platform's
// default filesystem (NTFS is case-insensitive-but-case-preserving): writing
// "Readme.txt" after "README.txt" exists does not create a second directory
// entry, it just rewrites the content of the existing "README.txt" entry (the
// directory listing then has only 2 entries, not 3, which is what actually
// failed here). The R-491-3 tie-break RULE itself is already fully pinned,
// without relying on an unreal filesystem precondition, by
// `compare_names_r491_3_tie_break_is_the_original_ordinal_comparison` above
// (a pure function test over the strings directly). This test is narrowed to
// what the real filesystem can represent: plain case-insensitive ordering
// with no forced tie, still exercising `build_sibling_list`'s use of
// `compare_names` as its sort key end-to-end.
#[test]
fn ac490_1_the_full_list_is_sorted_with_compare_names_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "a.txt");
    touch(dir.path(), "README.txt");

    let list = build_sibling_list(dir.path()).expect("build_sibling_list");
    let names = names_of(&list);

    assert_eq!(names, vec!["a.txt", "README.txt"]);
}

#[test]
fn an_empty_directory_yields_an_empty_list_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let list = build_sibling_list(dir.path()).expect("build_sibling_list");
    assert!(list.is_empty());
}

#[test]
fn a_missing_directory_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist");
    assert!(build_sibling_list(&missing).is_err());
}
