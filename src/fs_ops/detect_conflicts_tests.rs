//! TDD (unit-test-writer, 2026-09-26): `transfer::detect_conflicts` and the
//! `itemResolutions` validation rules.
//!
//! Specs: SPEC-0300-0011 (4th ed.) R-25, R-30-2, R-31-3, R-32, R-34-2,
//! R-37-1, R-37-2, R-37-4, R-24, R-85-1, AC-20-2, AC-29-1, AC-78~AC-81;
//! design: SPEC-0200-0001 §3.3.7, §3.3.13-2, §3.3.13-7 (SHOULD-82),
//! §3.3.14-2 (top-level items only).
//!
//! Expected API (stubs exist; bodies are `unimplemented!()`):
//! `transfer::detect_conflicts(&[PathBuf], &Path) -> Result<Vec<Conflict>,
//! TransferPreflightError>`, `commands::resolve_item_resolution`.

use std::collections::HashMap;
use std::path::PathBuf;

use super::guarded::resolve_item_resolution;
use super::preflight::TransferPreflightError;
use super::test_support::*;
use super::transfer::{
    copy_batch_with_item_resolutions, detect_conflicts, ConflictKind, ConflictResolution,
};

fn conflict_names(items: &[PathBuf], destination: &std::path::Path) -> Vec<String> {
    detect_conflicts(items, destination)
        .expect("detect_conflicts should succeed")
        .into_iter()
        .map(|c| c.name)
        .collect()
}

// R-32 / AC-29-1: a source whose name exists directly under the destination
// is reported, with its source path and name; others are not.
#[test]
fn a_top_level_name_conflict_is_reported_with_path_and_name() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "a");
    let b = write(root.path(), "src/b.txt", "b");
    write(root.path(), "dest/a.txt", "old-a");

    let conflicts = detect_conflicts(&[a.clone(), b], &root.path().join("dest")).expect("ok");

    assert_eq!(conflicts.len(), 1, "only a.txt conflicts");
    assert_eq!(conflicts[0].path, a);
    assert_eq!(conflicts[0].name, "a.txt");
    assert_eq!(conflicts[0].kind, ConflictKind::Overwritable);
}

// No conflicts -> empty vector (not an error).
#[test]
fn no_conflicts_yields_an_empty_list() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "a");
    std::fs::create_dir_all(root.path().join("dest")).unwrap();

    assert!(conflict_names(&[a], &root.path().join("dest")).is_empty());
}

// R-31-3 / SPEC-0200-0001 §3.3.14-2: only TOP-LEVEL items are detected. A
// same-named file nested inside a folder is NOT reported; the folder itself
// (top-level) is.
#[test]
fn r31_3_only_top_level_names_are_detected_not_files_inside_folders() {
    let root = tempdir();
    let folder = write(root.path(), "src/d/inner.txt", "new")
        .parent()
        .unwrap()
        .to_path_buf();
    let file = write(root.path(), "src/a.txt", "a");
    // dest has d/inner.txt (nested same name) and d itself, but no top-level a.txt
    write(root.path(), "dest/d/inner.txt", "old");
    write(
        root.path(),
        "dest/other/a.txt",
        "nested a.txt must not count",
    );

    let names = conflict_names(&[file, folder], &root.path().join("dest"));

    assert_eq!(
        names,
        vec!["d".to_string()],
        "only the top-level folder `d` conflicts, not inner.txt / other/a.txt"
    );
}

// Order of the reported conflicts follows the order of `items`.
#[test]
fn conflicts_keep_the_order_of_the_input_items() {
    let root = tempdir();
    let x = write(root.path(), "src/x.txt", "x");
    let y = write(root.path(), "src/y.txt", "y");
    let z = write(root.path(), "src/z.txt", "z");
    write(root.path(), "dest/x.txt", "1");
    write(root.path(), "dest/z.txt", "1");

    assert_eq!(
        conflict_names(&[z, y, x], &root.path().join("dest")),
        vec!["z.txt", "x.txt"]
    );
}

// R-32-2 (Windows): the name comparison follows the OS (case-insensitive on
// NTFS), i.e. `Path::exists`-like; no custom comparison rule.
#[cfg(windows)]
#[test]
fn r32_name_conflict_follows_os_case_rules_on_windows() {
    let root = tempdir();
    let a = write(root.path(), "src/REPORT.TXT", "new");
    write(root.path(), "dest/report.txt", "old");

    assert_eq!(
        conflict_names(&[a], &root.path().join("dest")),
        vec!["REPORT.TXT"]
    );
}

// Detection has zero side effects (both trees unchanged).
#[test]
fn detection_has_no_side_effects() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "a");
    write(root.path(), "dest/a.txt", "old");
    let before_src = names(&root.path().join("src"));
    let before_dest = names(&root.path().join("dest"));

    let _ = detect_conflicts(&[a.clone()], &root.path().join("dest"));

    assert_eq!(names(&root.path().join("src")), before_src);
    assert_eq!(names(&root.path().join("dest")), before_dest);
    assert_eq!(read(&a), "a");
    assert_eq!(read(&root.path().join("dest/a.txt")), "old");
}

// R-24 / R-83: destination problems use the same codes as preflight.
#[test]
fn destination_missing_or_not_a_directory_is_an_error_with_preflight_codes() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "a");
    let file_dest = write(root.path(), "not_a_dir.txt", "x");

    assert_eq!(
        detect_conflicts(&[a.clone()], &root.path().join("missing")).unwrap_err(),
        TransferPreflightError::DestinationNotFound
    );
    assert_eq!(
        detect_conflicts(&[a], &file_dest).unwrap_err(),
        TransferPreflightError::DestinationNotDir
    );
}

// R-25 / AC-14: pasting into the item's own directory is the same item ->
// excluded (skipped), never a conflict.
#[test]
fn r25_the_same_item_is_not_a_conflict() {
    let root = tempdir();
    let a = write(root.path(), "dir/a.txt", "a");

    assert!(conflict_names(&[a], &root.path().join("dir")).is_empty());
}

// R-37-1 / AC-78 (Windows): the destination spelled with different letter
// case is the SAME directory -> the item is the same item, not a conflict
// (otherwise overwrite would delete the source).
#[cfg(windows)]
#[test]
fn ac78_same_directory_spelled_with_different_case_is_not_a_conflict() {
    let root = tempdir();
    let a = write(root.path(), "photos/a.txt", "a");
    let dest_upper = with_last_component_uppercased(&root.path().join("photos"));

    assert!(conflict_names(&[a], &dest_upper).is_empty());
}

// R-37-1 / AC-79: a junction (symlink on unix) to the same directory is the
// same entity -> not a conflict.
#[test]
fn ac79_same_directory_via_a_junction_is_not_a_conflict() {
    let root = tempdir();
    let a = write(root.path(), "real/a.txt", "a");
    let link = root.path().join("link");
    assert!(
        make_dir_link(&link, &root.path().join("real")),
        "test setup: could not create a directory link"
    );

    let result = conflict_names(&[a], &link);

    remove_dir_link(&link);
    assert!(
        result.is_empty(),
        "same entity via junction must not be reported: {result:?}"
    );
}

// R-37-1 / AC-80: canonical (verbatim `\\?\`) and plain spellings of the
// same directory are the same entity.
#[test]
fn ac80_canonical_and_plain_spellings_of_the_same_directory_are_the_same_item() {
    let root = tempdir();
    let a = write(root.path(), "dir/a.txt", "a");
    let canonical_dest = std::fs::canonicalize(root.path().join("dir")).expect("canonicalize");

    assert!(conflict_names(&[a], &canonical_dest).is_empty());
}

// R-37-2 / R-37-4 / R-85-1 / AC-81: the source lives beneath the existing
// destination item (X/foo/foo pasted into X, X/foo exists) -> reported with
// the non-overwritable kind, so the UI offers no "overwrite".
#[test]
fn ac81_source_beneath_the_conflicting_destination_item_is_overwrite_ancestor() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo/leaf.txt", "leaf")
        .parent()
        .unwrap()
        .to_path_buf(); // X/foo/foo (dir)

    let conflicts = detect_conflicts(&[inner.clone()], &root.path().join("X")).expect("ok");

    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, inner);
    assert_eq!(conflicts[0].name, "foo");
    assert_eq!(conflicts[0].kind, ConflictKind::OverwriteAncestor);
}

// AC-81 (file variant): X/foo/foo is a FILE and X/foo a folder; the
// ancestor relation takes precedence over a plain kind mismatch.
#[test]
fn ac81_file_source_beneath_the_destination_folder_is_also_overwrite_ancestor() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo", "file");

    let conflicts = detect_conflicts(&[inner], &root.path().join("X")).expect("ok");

    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].kind, ConflictKind::OverwriteAncestor);
}

// AC-81 via a junction alias of X (identity by canonical form, R-37-1).
#[test]
fn ac81_ancestor_detection_works_through_a_junction_alias_of_the_destination() {
    let root = tempdir();
    let inner = write(root.path(), "X/foo/foo", "file");
    let alias = root.path().join("alias_of_x");
    assert!(
        make_dir_link(&alias, &root.path().join("X")),
        "test setup: could not create a directory link"
    );

    let conflicts = detect_conflicts(&[inner], &alias).expect("ok");

    remove_dir_link(&alias);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].kind, ConflictKind::OverwriteAncestor);
}

// ------------------------------------------------------------------
// itemResolutions validation (SHOULD-82; R-30-2, R-34-2)
// ------------------------------------------------------------------

fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// A per-item value wins over the batch default.
#[test]
fn item_resolution_overrides_the_default_for_that_item() {
    let m = map(&[("C:\\src\\a.txt", "overwrite"), ("C:\\src\\b.txt", "skip")]);
    assert_eq!(
        resolve_item_resolution("C:\\src\\a.txt", ConflictResolution::Reject, Some(&m)),
        ConflictResolution::Overwrite
    );
    assert_eq!(
        resolve_item_resolution("C:\\src\\b.txt", ConflictResolution::Reject, Some(&m)),
        ConflictResolution::Skip
    );
}

// No entry (or no map at all) -> the default applies; the default itself is
// `reject` on the non-UI path (R-30-2).
#[test]
fn items_without_an_entry_use_the_default_resolution() {
    let m = map(&[("C:\\src\\a.txt", "overwrite")]);
    assert_eq!(
        resolve_item_resolution("C:\\src\\other.txt", ConflictResolution::Reject, Some(&m)),
        ConflictResolution::Reject
    );
    assert_eq!(
        resolve_item_resolution("C:\\src\\a.txt", ConflictResolution::Skip, None),
        ConflictResolution::Skip
    );
}

// SHOULD-82: an unknown value is treated as `reject` (never as overwrite),
// even when the default is more permissive.
#[test]
fn an_unknown_value_is_reject() {
    let m = map(&[
        ("a", "OVERWRITE"),
        ("b", "replace"),
        ("c", ""),
        ("d", "delete"),
    ]);
    for key in ["a", "b", "c", "d"] {
        assert_eq!(
            resolve_item_resolution(key, ConflictResolution::Overwrite, Some(&m)),
            ConflictResolution::Reject,
            "unknown value for key {key} must be reject"
        );
    }
}

// SHOULD-82: keys are matched by EXACT string equality with the item
// (no case folding, no trailing-separator or slash normalization).
#[test]
fn keys_must_match_the_item_string_exactly() {
    let m = map(&[("C:\\src\\a.txt", "overwrite")]);
    for near_miss in [
        "c:\\src\\a.txt",
        "C:\\src\\A.TXT",
        "C:/src/a.txt",
        "C:\\src\\a.txt\\",
        " C:\\src\\a.txt",
    ] {
        assert_eq!(
            resolve_item_resolution(near_miss, ConflictResolution::Reject, Some(&m)),
            ConflictResolution::Reject,
            "`{near_miss}` is not an exact match and must fall back to the default"
        );
    }
}

// SHOULD-82: keys that are not among `items` are ignored -- they neither
// affect other items nor become extra operations.
#[test]
fn keys_that_are_not_items_are_ignored_by_the_batch() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "new-a");
    let b = write(root.path(), "src/b.txt", "new-b");
    write(root.path(), "dest/a.txt", "old-a");
    write(root.path(), "dest/b.txt", "old-b");
    let stray = root.path().join("src").join("not_an_item.txt");
    let mut per_item = HashMap::new();
    per_item.insert(a.clone(), ConflictResolution::Overwrite);
    per_item.insert(stray, ConflictResolution::Overwrite);

    let summary = copy_batch_with_item_resolutions(
        &[a, b],
        &root.path().join("dest"),
        ConflictResolution::Reject,
        &per_item,
    );

    assert_eq!(summary.total, 2);
    assert_eq!(summary.ok, 1, "a.txt overwritten");
    assert_eq!(
        summary.failed, 1,
        "b.txt has no entry -> default reject -> conflict"
    );
    assert_eq!(summary.failures[0].code, "conflict");
    assert_eq!(read(&root.path().join("dest/a.txt")), "new-a");
    assert_eq!(
        read(&root.path().join("dest/b.txt")),
        "old-b",
        "b.txt must not be silently overwritten (R-30-2)"
    );
    assert!(!root.path().join("dest/not_an_item.txt").exists());
}

// R-34 / AC-27-equivalent: one batch can overwrite one item and skip
// another (per-item resolutions), evaluated at processing time.
#[test]
fn per_item_resolutions_can_overwrite_one_item_and_skip_another() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "new-a");
    let b = write(root.path(), "src/b.txt", "new-b");
    write(root.path(), "dest/a.txt", "old-a");
    write(root.path(), "dest/b.txt", "old-b");
    let mut per_item = HashMap::new();
    per_item.insert(a.clone(), ConflictResolution::Overwrite);
    per_item.insert(b.clone(), ConflictResolution::Skip);

    let summary = copy_batch_with_item_resolutions(
        &[a, b],
        &root.path().join("dest"),
        ConflictResolution::Reject,
        &per_item,
    );

    assert_eq!((summary.ok, summary.skipped, summary.failed), (1, 1, 0));
    assert_eq!(read(&root.path().join("dest/a.txt")), "new-a");
    assert_eq!(read(&root.path().join("dest/b.txt")), "old-b");
}
