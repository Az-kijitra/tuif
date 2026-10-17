//! TDD (unit-test-writer, 2026-09-26): "overwrite" = file replace + folder
//! MERGE (SPEC-0300-0011 4th ed. R-31, R-31-2~R-31-7, R-80; AC-20, AC-20-1,
//! AC-20-3, AC-20-4, AC-20-6; design SPEC-0200-0001 §3.3.14-2).
//!
//! The current implementation deletes the destination first (`remove_dir_all`)
//! and then transfers; these tests demand the merge behaviour instead.
//! Only the existing public API is used (`copy_batch` / `move_batch`).

use super::test_support::*;
use super::transfer::{copy_batch, move_batch, ConflictResolution};

// AC-20-1 / R-31 / R-31-2: copying a folder over an existing folder with
// "overwrite" MERGES: same-named files are replaced, destination-only files
// stay, new files arrive; nested folders merge recursively. The copy source
// is untouched (R-37).
#[test]
fn ac20_1_copy_overwrite_merges_folders_and_keeps_destination_only_files() {
    let root = tempdir();
    let src = write(root.path(), "src/d/both.txt", "NEW").parent().unwrap().parent().unwrap().join("d");
    write(root.path(), "src/d/new.txt", "brand-new");
    write(root.path(), "src/d/sub/deep_both.txt", "NEW-DEEP");
    write(root.path(), "dest/d/both.txt", "OLD");
    write(root.path(), "dest/d/only_dest.txt", "keep-me");
    write(root.path(), "dest/d/sub/deep_only_dest.txt", "keep-deep");
    write(root.path(), "dest/d/sub/deep_both.txt", "OLD-DEEP");

    let summary = copy_batch(&[src.clone()], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (1, 0), "{:?}", summary.failures);
    let d = root.path().join("dest/d");
    assert_eq!(read(&d.join("both.txt")), "NEW", "same-named file replaced");
    assert_eq!(read(&d.join("only_dest.txt")), "keep-me", "destination-only file must remain (merge, not replace)");
    assert_eq!(read(&d.join("new.txt")), "brand-new");
    assert_eq!(read(&d.join("sub/deep_both.txt")), "NEW-DEEP");
    assert_eq!(read(&d.join("sub/deep_only_dest.txt")), "keep-deep", "nested destination-only file must remain");
    // source intact
    assert_eq!(read(&src.join("both.txt")), "NEW");
    assert_eq!(read(&src.join("sub/deep_both.txt")), "NEW-DEEP");
}

// AC-20-1 (move) + R-31-7 / AC-20-3: same merge result; every item of the
// source folder transferred -> the (now empty) source folder is deleted.
#[test]
fn ac20_1_and_ac20_3_move_overwrite_merges_and_removes_the_emptied_source_folder() {
    let root = tempdir();
    write(root.path(), "src/d/both.txt", "NEW");
    write(root.path(), "src/d/sub/deep.txt", "NEW-DEEP");
    write(root.path(), "dest/d/both.txt", "OLD");
    write(root.path(), "dest/d/only_dest.txt", "keep-me");

    let summary = move_batch(&[root.path().join("src/d")], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (1, 0), "{:?}", summary.failures);
    let d = root.path().join("dest/d");
    assert_eq!(read(&d.join("both.txt")), "NEW");
    assert_eq!(read(&d.join("only_dest.txt")), "keep-me");
    assert_eq!(read(&d.join("sub/deep.txt")), "NEW-DEEP");
    assert!(!root.path().join("src/d").exists(), "R-31-7: emptied source folder must be removed");
}

// R-31-2 / R-31-5: overwriting a FILE replaces just that file, leaves the
// rest of the destination alone, and leaves no temporary file behind.
#[test]
fn r31_5_file_overwrite_replaces_content_and_leaves_no_stray_temp_files() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "NEW");
    write(root.path(), "dest/a.txt", "OLD");
    write(root.path(), "dest/other.txt", "other");

    let summary = copy_batch(&[a], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (1, 0));
    assert_eq!(read(&root.path().join("dest/a.txt")), "NEW");
    assert_eq!(names(&root.path().join("dest")), vec!["a.txt", "other.txt"], "no temp/backup leftovers");
}

// Same for move: the source file is gone, destination replaced, no leftovers.
#[test]
fn r31_5_move_file_overwrite_replaces_destination_and_removes_source_without_leftovers() {
    let root = tempdir();
    let a = write(root.path(), "src/a.txt", "NEW");
    write(root.path(), "dest/a.txt", "OLD");

    let summary = move_batch(&[a.clone()], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (1, 0));
    assert!(!a.exists());
    assert_eq!(read(&root.path().join("dest/a.txt")), "NEW");
    assert_eq!(names(&root.path().join("dest")), vec!["a.txt"]);
}

// R-31-4 / R-31-5 / AC-20-4 / AC-20-6 (inside a merge): a file in the source
// folder faces a same-named FOLDER in the destination. That item fails with
// `conflict` and the destination entry is untouched; the merge of everything
// else still happens; and -- crucially -- the destination folder was NOT
// deleted up front, so destination-only data survives the mid-way failure.
#[test]
fn r31_4_r31_5_kind_mismatch_inside_a_merge_fails_without_losing_destination_data() {
    let root = tempdir();
    let src = write(root.path(), "src/d/x", "i-am-a-file").parent().unwrap().to_path_buf();
    write(root.path(), "src/d/ok.txt", "NEW-OK");
    write(root.path(), "dest/d/x/precious.txt", "precious");
    write(root.path(), "dest/d/only_dest.txt", "keep-me");

    let summary = copy_batch(&[src], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!(summary.failed, 1, "the folder item reports a failure: {summary:?}");
    assert!(summary.failures.iter().any(|f| f.code == "conflict"), "R-31-4 provisional code is `conflict`: {summary:?}");
    let d = root.path().join("dest/d");
    assert_eq!(read(&d.join("x/precious.txt")), "precious", "destination folder content must be unchanged");
    assert!(d.join("x").is_dir(), "destination folder must not be replaced by the file");
    assert_eq!(read(&d.join("only_dest.txt")), "keep-me", "R-31-5: no up-front deletion of the destination folder");
    assert_eq!(read(&d.join("ok.txt")), "NEW-OK", "R-80: the rest of the merge continues");
}

// R-31-4 (reverse, inside a merge): source has a FOLDER where the destination
// has a FILE with the same name.
#[test]
fn r31_4_folder_over_file_inside_a_merge_fails_and_keeps_the_destination_file() {
    let root = tempdir();
    let src = write(root.path(), "src/d/x/inner.txt", "inner").parent().unwrap().parent().unwrap().to_path_buf();
    write(root.path(), "dest/d/x", "i-am-a-dest-file");

    let summary = copy_batch(&[src], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!(summary.failed, 1, "{summary:?}");
    assert_eq!(summary.failures[0].code, "conflict");
    assert_eq!(read(&root.path().join("dest/d/x")), "i-am-a-dest-file");
}

// R-31-4 / AC-20-6 (top level, copy): file -> same-named folder fails with
// `conflict`, the destination folder is unchanged, and the next item of the
// batch is still processed (R-80).
#[test]
fn ac20_6_top_level_file_over_folder_fails_with_conflict_and_the_batch_continues() {
    let root = tempdir();
    let file_item = write(root.path(), "src/x", "file");
    let next = write(root.path(), "src/next.txt", "next");
    write(root.path(), "dest/x/precious.txt", "precious");

    let summary = copy_batch(&[file_item, next], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed, summary.total), (1, 1, 2), "{summary:?}");
    assert_eq!(summary.failures[0].code, "conflict");
    assert_eq!(read(&root.path().join("dest/x/precious.txt")), "precious");
    assert!(root.path().join("dest/x").is_dir());
    assert_eq!(read(&root.path().join("dest/next.txt")), "next");
}

// R-31-4 / AC-20-6 (top level, copy): folder -> same-named file.
#[test]
fn ac20_6_top_level_folder_over_file_fails_with_conflict_and_keeps_the_destination_file() {
    let root = tempdir();
    let folder = write(root.path(), "src/x/inner.txt", "inner").parent().unwrap().to_path_buf();
    write(root.path(), "dest/x", "dest-file");

    let summary = copy_batch(&[folder], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (0, 1), "{summary:?}");
    assert_eq!(summary.failures[0].code, "conflict");
    assert_eq!(read(&root.path().join("dest/x")), "dest-file");
}

// R-31-4 + R-37 (move): a kind mismatch must never delete the SOURCE either.
#[test]
fn r31_4_move_with_kind_mismatch_keeps_both_source_and_destination() {
    let root = tempdir();
    let file_item = write(root.path(), "src/x", "file");
    write(root.path(), "dest/x/precious.txt", "precious");

    let summary = move_batch(&[file_item.clone()], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!((summary.ok, summary.failed), (0, 1), "{summary:?}");
    assert_eq!(summary.failures[0].code, "conflict");
    assert_eq!(read(&file_item), "file", "source must remain");
    assert_eq!(read(&root.path().join("dest/x/precious.txt")), "precious");
}

// R-31-7 / AC-20-3 (partial failure): when part of a moved folder cannot be
// transferred (here: kind mismatch), the successfully transferred items are
// removed from the source, but the failed item AND the folder containing it
// remain; the failure is listed (R-80).
#[test]
fn ac20_3_move_merge_with_a_failed_item_keeps_the_failed_item_and_its_source_folder() {
    let root = tempdir();
    write(root.path(), "src/d/moved.txt", "moved");
    write(root.path(), "src/d/x", "i-am-a-file");
    write(root.path(), "dest/d/x/precious.txt", "precious");

    let summary = move_batch(&[root.path().join("src/d")], &root.path().join("dest"), ConflictResolution::Overwrite);

    assert_eq!(summary.failed, 1, "{summary:?}");
    assert_eq!(summary.failures[0].code, "conflict");
    assert!(root.path().join("src/d").is_dir(), "source folder holding the failed item must remain");
    assert_eq!(read(&root.path().join("src/d/x")), "i-am-a-file", "failed item stays in the source");
    assert!(!root.path().join("src/d/moved.txt").exists(), "successfully transferred item is removed from the source");
    assert_eq!(read(&root.path().join("dest/d/moved.txt")), "moved");
    assert_eq!(read(&root.path().join("dest/d/x/precious.txt")), "precious");
}

// R-31-5 / AC-20-4 (Windows): a destination file that cannot be replaced
// (read-only) makes that ONE item fail, but its ORIGINAL content survives
// (no delete-then-copy), destination-only files remain, and the other
// files of the merge are still processed.
#[cfg(windows)]
#[test]
fn ac20_4_a_replace_failure_keeps_the_original_destination_file() {
    let root = tempdir();
    let src = write(root.path(), "src/d/a_first.txt", "NEW-FIRST").parent().unwrap().to_path_buf();
    write(root.path(), "src/d/locked.txt", "NEW-LOCKED");
    let locked = write(root.path(), "dest/d/locked.txt", "ORIGINAL");
    write(root.path(), "dest/d/only_dest.txt", "keep-me");
    let mut perms = std::fs::metadata(&locked).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&locked, perms).unwrap();

    let summary = copy_batch(&[src], &root.path().join("dest"), ConflictResolution::Overwrite);

    // make the temp dir removable again before asserting
    let mut perms = std::fs::metadata(&locked).unwrap().permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(&locked, perms).unwrap();

    assert_eq!(summary.failed, 1, "{summary:?}");
    assert_eq!(read(&locked), "ORIGINAL", "R-31-5: the original destination data must survive a failed replace");
    assert_eq!(read(&root.path().join("dest/d/only_dest.txt")), "keep-me");
    assert_eq!(read(&root.path().join("dest/d/a_first.txt")), "NEW-FIRST", "R-80: the rest continues");
    let leftovers: Vec<String> =
        names(&root.path().join("dest/d")).into_iter().filter(|n| !["a_first.txt", "locked.txt", "only_dest.txt"].contains(&n.as_str())).collect();
    assert!(leftovers.is_empty(), "no temp files left after a failed replace: {leftovers:?}");
}

// R-31-2: "skip" is unaffected -- an existing destination folder is left
// exactly as it was (nothing merged), counted as skipped.
#[test]
fn skip_resolution_does_not_merge_into_an_existing_folder() {
    let root = tempdir();
    let src = write(root.path(), "src/d/new.txt", "new").parent().unwrap().to_path_buf();
    write(root.path(), "dest/d/old.txt", "old");

    let summary = copy_batch(&[src], &root.path().join("dest"), ConflictResolution::Skip);

    assert_eq!((summary.ok, summary.skipped, summary.failed), (0, 1, 0));
    assert_eq!(names(&root.path().join("dest/d")), vec!["old.txt"]);
}
