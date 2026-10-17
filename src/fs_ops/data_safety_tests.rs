//! TDD (unit-test-writer, 2026-09-26): data safety of copy/move/overwrite
//! (MUST-59, the most important requirement of this phase).
//!
//! Specs: SPEC-0300-0011 (4th ed.) R-37, R-37-1, R-37-2, R-25, R-24, R-85-1,
//! AC-78, AC-79, AC-80, AC-81, AC-82; design SPEC-0200-0001 §3.3.13-2.
//!
//! Invariant under test: whatever alias (letter case, junction/symlink,
//! canonical `\\?\` spelling) is used, overwrite/merge must NEVER delete or
//! replace the copy/move SOURCE, and the identity check must also be used by
//! `preflight_transfer` (no string-only `==` / `starts_with`).

use super::preflight::{preflight_transfer, TransferPreflightError};
use super::test_support::*;
use super::transfer::{copy_batch, move_batch, ConflictResolution};

// ---------- same item through an alias (R-37-1 -> R-25 skipped) ----------

// AC-78 (Windows), copy: destination spelled with different case = the same
// directory. The item is excluded as R-25 "same item" (skipped) and the source
// file keeps its content (the old code deleted it as the "existing target").
#[cfg(windows)]
#[test]
fn ac78_copy_overwrite_into_the_same_dir_with_different_case_keeps_the_source() {
    let root = tempdir();
    let a = write(root.path(), "photos/a.txt", "PRECIOUS");
    let dest_upper = with_last_component_uppercased(&root.path().join("photos"));

    let summary = copy_batch(&[a.clone()], &dest_upper, ConflictResolution::Overwrite);

    assert_eq!(read(&a), "PRECIOUS", "the source must survive");
    assert_eq!((summary.skipped, summary.failed), (1, 0), "R-25: same item is skipped, not a failure: {summary:?}");
}

// AC-78 (Windows), move.
#[cfg(windows)]
#[test]
fn ac78_move_overwrite_into_the_same_dir_with_different_case_keeps_the_source() {
    let root = tempdir();
    let a = write(root.path(), "photos/a.txt", "PRECIOUS");
    let dest_upper = with_last_component_uppercased(&root.path().join("photos"));

    let summary = move_batch(&[a.clone()], &dest_upper, ConflictResolution::Overwrite);

    assert_eq!(read(&a), "PRECIOUS");
    assert_eq!((summary.skipped, summary.failed), (1, 0), "{summary:?}");
}

// R-25 before R-30: with the default `reject`, the same item through a
// different-case spelling is still "skipped", not a `conflict` failure.
#[cfg(windows)]
#[test]
fn r25_same_item_via_different_case_is_skipped_even_with_reject() {
    let root = tempdir();
    let a = write(root.path(), "photos/a.txt", "x");
    let dest_upper = with_last_component_uppercased(&root.path().join("photos"));

    let summary = copy_batch(&[a], &dest_upper, ConflictResolution::Reject);

    assert_eq!((summary.skipped, summary.failed), (1, 0), "{summary:?}");
}

// AC-79, copy: destination is a junction (symlink) to the source's own
// directory -> same entity; the source file survives an "overwrite".
#[test]
fn ac79_copy_overwrite_via_a_junction_to_the_same_dir_keeps_the_source() {
    let root = tempdir();
    let a = write(root.path(), "real/a.txt", "PRECIOUS");
    let link = root.path().join("link");
    assert!(make_dir_link(&link, &root.path().join("real")), "test setup: could not create a directory link");

    let summary = copy_batch(&[a.clone()], &link, ConflictResolution::Overwrite);

    remove_dir_link(&link);
    assert_eq!(read(&a), "PRECIOUS");
    assert_eq!(summary.failed, 0, "{summary:?}");
    assert_eq!(summary.skipped, 1, "same entity -> R-25 skipped: {summary:?}");
}

// AC-79, move (file).
#[test]
fn ac79_move_overwrite_via_a_junction_to_the_same_dir_keeps_the_source() {
    let root = tempdir();
    let a = write(root.path(), "real/a.txt", "PRECIOUS");
    let link = root.path().join("link");
    assert!(make_dir_link(&link, &root.path().join("real")), "test setup: could not create a directory link");

    let summary = move_batch(&[a.clone()], &link, ConflictResolution::Overwrite);

    remove_dir_link(&link);
    assert_eq!(read(&a), "PRECIOUS");
    assert_eq!(summary.failed, 0, "{summary:?}");
}

// AC-79 (folder item): source folder real/sub, destination is a junction to
// `real`, so the target link/sub is real/sub itself. Neither the folder nor
// its contents may be removed (the old `remove_dir_all(target)` erased it).
#[test]
fn ac79_folder_overwrite_via_a_junction_keeps_the_source_folder_and_its_files() {
    let root = tempdir();
    write(root.path(), "real/sub/f.txt", "PRECIOUS");
    let link = root.path().join("link");
    assert!(make_dir_link(&link, &root.path().join("real")), "test setup: could not create a directory link");

    let copy = copy_batch(&[root.path().join("real/sub")], &link, ConflictResolution::Overwrite);
    let mv = move_batch(&[root.path().join("real/sub")], &link, ConflictResolution::Overwrite);

    remove_dir_link(&link);
    assert_eq!(read(&root.path().join("real/sub/f.txt")), "PRECIOUS");
    assert_eq!((copy.failed, mv.failed), (0, 0), "{copy:?} {mv:?}");
}

// ---------- source beneath the overwrite target (R-37-2, R-85-1) ----------

// AC-81 (folder source, copy): X/foo/foo pasted into X while X/foo exists.
// Overwriting X/foo would destroy the source -> refuse with
// `overwrite_ancestor`; neither the source nor the existing X/foo changes.
#[test]
fn ac81_copy_overwrite_of_an_ancestor_is_refused_and_nothing_changes() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo/leaf.txt", "PRECIOUS").parent().unwrap().to_path_buf();
    write(root.path(), "X/foo/sibling.txt", "sibling");
    let before_foo = names(&root.path().join("X/foo"));

    let summary = copy_batch(&[inner.clone()], &root.path().join("X"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (0, 1), "{summary:?}");
    assert_eq!(summary.failures[0].code, "overwrite_ancestor");
    assert_eq!(read(&inner.join("leaf.txt")), "PRECIOUS", "the copy source must remain");
    assert_eq!(read(&root.path().join("X/foo/sibling.txt")), "sibling");
    assert_eq!(names(&root.path().join("X/foo")), before_foo, "the existing destination item is unchanged");
}

// AC-81 (file source, move).
#[test]
fn ac81_move_overwrite_of_an_ancestor_is_refused_and_the_source_remains() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo", "PRECIOUS");
    write(root.path(), "X/foo/sibling.txt", "sibling");

    let summary = move_batch(&[inner.clone()], &root.path().join("X"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (0, 1), "{summary:?}");
    assert_eq!(summary.failures[0].code, "overwrite_ancestor");
    assert_eq!(read(&inner), "PRECIOUS");
    assert_eq!(read(&root.path().join("X/foo/sibling.txt")), "sibling");
}

// R-37-2 + R-80: the refusal is per item; the rest of the batch continues.
#[test]
fn ac81_an_ancestor_refusal_does_not_stop_the_other_items() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo", "PRECIOUS");
    let other = write(root.path(), "elsewhere/other.txt", "other");

    let summary = copy_batch(&[inner.clone(), other], &root.path().join("X"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed, summary.total), (1, 1, 2), "{summary:?}");
    assert_eq!(read(&root.path().join("X/other.txt")), "other");
    assert_eq!(read(&inner), "PRECIOUS");
}

// AC-81 via a junction alias of X (identity by canonical form).
#[test]
fn ac81_ancestor_is_recognised_through_a_junction_alias_of_the_destination() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo", "PRECIOUS");
    let alias = root.path().join("alias_of_x");
    assert!(make_dir_link(&alias, &root.path().join("X")), "test setup: could not create a directory link");

    let summary = copy_batch(&[inner.clone()], &alias, ConflictResolution::Overwrite);

    remove_dir_link(&alias);
    assert_eq!(read(&inner), "PRECIOUS");
    assert_eq!(summary.failed, 1, "{summary:?}");
    assert_eq!(summary.failures[0].code, "overwrite_ancestor");
}

// ---------- preflight must use the same identity check (R-24, R-37-1) ----------

// AC-80: the canonical (`\\?\`-style) spelling of the very same directory is
// the same path -> `same_path`.
#[test]
fn ac80_preflight_same_path_is_detected_across_spellings() {
    let root = tempdir();
    let dir = root.path().join("dir");
    std::fs::create_dir_all(&dir).unwrap();
    let canonical = std::fs::canonicalize(&dir).expect("canonicalize");

    assert_eq!(preflight_transfer(&[dir], &canonical), Err(TransferPreflightError::SamePath));
}

// AC-82 (Windows): destination inside the source, written with different
// letter case -> `destination_inside_source` (would otherwise recurse).
#[cfg(windows)]
#[test]
fn ac82_preflight_detects_destination_inside_source_with_different_case() {
    let root = tempdir();
    let source = root.path().join("parent");
    std::fs::create_dir_all(source.join("child")).unwrap();
    let dest_other_case = with_last_component_uppercased(&source).join("child");

    assert_eq!(
        preflight_transfer(&[source], &dest_other_case),
        Err(TransferPreflightError::DestinationInsideSource)
    );
}

// AC-82: destination reached through a junction that points INSIDE the source.
#[test]
fn ac82_preflight_detects_destination_inside_source_via_a_junction() {
    let root = tempdir();
    let source = root.path().join("parent");
    std::fs::create_dir_all(source.join("child")).unwrap();
    let link = root.path().join("link_to_child");
    assert!(make_dir_link(&link, &source.join("child")), "test setup: could not create a directory link");

    let result = preflight_transfer(&[source], &link);

    remove_dir_link(&link);
    assert_eq!(result, Err(TransferPreflightError::DestinationInsideSource));
}

// AC-82: the canonical spelling of a directory inside the source is inside.
#[test]
fn ac82_preflight_detects_destination_inside_source_with_canonical_spelling() {
    let root = tempdir();
    let source = root.path().join("parent");
    let child = source.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let canonical_child = std::fs::canonicalize(&child).expect("canonicalize");

    assert_eq!(
        preflight_transfer(&[source], &canonical_child),
        Err(TransferPreflightError::DestinationInsideSource)
    );
}

// AC-79/AC-80: destination is a junction to the item itself -> `same_path`.
#[test]
fn ac80_preflight_same_path_is_detected_via_a_junction() {
    let root = tempdir();
    let real = root.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = root.path().join("link");
    assert!(make_dir_link(&link, &real), "test setup: could not create a directory link");

    let result = preflight_transfer(&[real], &link);

    remove_dir_link(&link);
    assert_eq!(result, Err(TransferPreflightError::SamePath));
}

// Regression guard: an unrelated sibling directory with a similar prefix is
// still fine (the identity check must not turn into a string-prefix match).
#[test]
fn preflight_does_not_confuse_a_sibling_with_a_similar_name_for_a_descendant() {
    let root = tempdir();
    let source = root.path().join("parent");
    let sibling = root.path().join("parent2");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    assert_eq!(preflight_transfer(&[source], &sibling), Ok(()));
}

// ---------- review fixes (M-1, M-2, minor 1, minor 2) ----------

// M-1: with `Reject` (what the UI sends for an ancestor conflict) the reported
// reason must be `overwrite_ancestor`, not a generic `conflict`; nothing changes.
#[test]
fn m1_reject_with_an_ancestor_source_reports_overwrite_ancestor() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo/leaf.txt", "PRECIOUS").parent().unwrap().to_path_buf();
    write(root.path(), "X/foo/sibling.txt", "sibling");
    let before_foo = names(&root.path().join("X/foo"));

    let summary = copy_batch(&[inner.clone()], &root.path().join("X"), ConflictResolution::Reject);

    assert_eq!((summary.ok, summary.failed), (0, 1), "{summary:?}");
    assert_eq!(summary.failures[0].code, "overwrite_ancestor");
    assert_eq!(read(&inner.join("leaf.txt")), "PRECIOUS");
    assert_eq!(names(&root.path().join("X/foo")), before_foo);
}

// M-1: an ordinary (non-ancestor) conflict with `Reject` is still `conflict`.
#[test]
fn m1_reject_with_an_ordinary_conflict_is_still_conflict() {
    let root = tempdir();
    let src = write(root.path(), "a.txt", "new");
    let dest = root.path().join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    write(&dest, "a.txt", "old");

    let summary = copy_batch(&[src], &dest, ConflictResolution::Reject);

    assert_eq!(summary.failures[0].code, "conflict");
    assert_eq!(read(&dest.join("a.txt")), "old");
}

// M-2: creating a file never truncates an entry that appeared in the meantime.
#[test]
fn m2_create_new_file_does_not_truncate_an_existing_file() {
    let root = tempdir();
    let existing = write(root.path(), "a.txt", "keep me");

    let result = super::create::create_new_file(&existing);

    assert_eq!(result.unwrap_err(), super::preflight::MutatePreflightError::AlreadyExists);
    assert_eq!(read(&existing), "keep me");
}

// minor 1: a case-only rename of the entry itself is allowed on Windows.
#[cfg(windows)]
#[test]
fn minor1_case_only_rename_is_allowed_on_windows() {
    let root = tempdir();
    let file = write(root.path(), "a.txt", "1");

    assert_eq!(super::preflight::preflight_rename(&file, "A.txt"), Ok(()));
    // A genuinely different existing sibling is still rejected.
    write(root.path(), "b.txt", "2");
    assert_eq!(
        super::preflight::preflight_rename(&file, "B.txt"),
        Err(super::preflight::MutatePreflightError::AlreadyExists)
    );
}

// minor 2: the move fallback must not overwrite a destination that appeared.
#[test]
fn minor2_move_fallback_refuses_an_existing_destination() {
    let root = tempdir();
    let src = write(root.path(), "s/a.txt", "src");
    let dst = write(root.path(), "d/a.txt", "existing");

    let err = super::transfer::move_fallback(&src, &dst).unwrap_err();

    assert_eq!(err.code, "conflict");
    assert_eq!(read(&dst), "existing");
    assert_eq!(read(&src), "src");
}

// ---------- Phase 11: R-24 / R-19-7 / AC-06-8 (paths from the OS clipboard may not exist) ----------
//
// An item that no longer exists is a per-item `not_found` failure; it never rejects the whole
// batch, and the remaining items are processed (the OS clipboard path goes through the same
// commands as the in-app one, SPEC-0200-0001 §3.10.10).

#[test]
fn ac06_8_copy_with_a_missing_item_in_the_middle_copies_the_rest_and_reports_not_found() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "A");
    let missing = root.path().join("src").join("zz.txt");
    let c = write(root.path(), "src/c.txt", "C");
    let dest = root.path().join("dst");
    std::fs::create_dir_all(&dest).unwrap();

    let summary = copy_batch(&[a, missing.clone(), c], &dest, ConflictResolution::Reject);

    assert_eq!((summary.total, summary.ok, summary.failed, summary.skipped), (3, 2, 1, 0), "{summary:?}");
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(summary.failures[0].path, missing);
    assert_eq!(summary.failures[0].code, "not_found");
    assert_eq!(read(&dest.join("a.txt")), "A");
    assert_eq!(read(&dest.join("c.txt")), "C");
}

#[test]
fn ac06_8_move_with_a_missing_item_moves_the_rest_and_reports_not_found() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "A");
    let missing = root.path().join("src").join("zz.txt");
    let dest = root.path().join("dst");
    std::fs::create_dir_all(&dest).unwrap();

    let summary = move_batch(&[missing.clone(), a.clone()], &dest, ConflictResolution::Reject);

    assert_eq!((summary.total, summary.ok, summary.failed), (2, 1, 1), "{summary:?}");
    assert_eq!(summary.failures[0].path, missing);
    assert_eq!(summary.failures[0].code, "not_found");
    assert_eq!(read(&dest.join("a.txt")), "A");
    assert!(!a.exists(), "the item that exists was moved");
}

// When every item is missing the batch still ends as per-item failures (no batch-level rejection).
#[test]
fn ac06_8_a_batch_of_only_missing_items_fails_each_item_with_not_found() {
    let root = tempdir();
    let dest = root.path().join("dst");
    std::fs::create_dir_all(&dest).unwrap();
    let items = [root.path().join("x1.txt"), root.path().join("x2.txt")];

    let summary = copy_batch(&items, &dest, ConflictResolution::Reject);

    assert_eq!((summary.total, summary.ok, summary.failed), (2, 0, 2), "{summary:?}");
    assert!(summary.failures.iter().all(|f| f.code == "not_found"));
}

// Conflict detection skips a missing item instead of erroring (R-24: `continue`).
#[test]
fn ac06_8_detect_conflicts_ignores_a_missing_item() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "A");
    write(root.path(), "dst/a.txt", "existing");
    let missing = root.path().join("src").join("zz.txt");

    let conflicts = super::transfer::detect_conflicts(&[missing, a], &root.path().join("dst")).expect("no batch-level error");

    assert_eq!(conflicts.len(), 1, "only a.txt conflicts");
    assert_eq!(conflicts[0].name, "a.txt");
}
