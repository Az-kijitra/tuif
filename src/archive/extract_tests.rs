//! TDD (unit-test-writer, 2026-09-27): `archive::extract` — the extraction run.
//!
//! Specs (SPEC-0300-0014): R-343-2, R-346 / R-346-1 / R-346-2 / R-346-3, R-348,
//! R-348-1, R-349 / R-349-1 (L-4, CRC), R-350, R-351-1, R-352, R-361-1, R-366,
//! R-367 / R-367-1, R-368, R-370, R-374 (resolutions), R-375, R-376, R-382,
//! R-384, R-385, R-387-2, R-395-2, R-365-3; AC-318, AC-319-1, AC-323, AC-324,
//! AC-328, AC-330, AC-331-1, AC-332, AC-343 .. AC-348, AC-354 .. AC-356,
//! AC-358-2, AC-364-2, AC-365, AC-365-1, AC-366, AC-368, AC-373, AC-373-1,
//! AC-374-1, AC-376, AC-380-1.
//! SPEC-0200-0001 §3.4.4 (execution), §3.4.10, §3.4.13-2 (MUST-66: commit is a
//! NO-REPLACE move unless the user chose "overwrite"), §3.4.14 C4 (CRC comes
//! from the zip crate; AC-373-1 verifies that it actually detects corruption).
//!
//! Outcome conventions used (SPEC-0200-0001 §3.4.2): `Err` = rejected before
//! anything was written; `Ok` with `Failed` / `Canceled` = writing had started
//! (already extracted entries stay). Where the spec allows either, the tests
//! use `code_of` (see `test_support`).

use super::error::{ArchiveError, ArchiveErrorCode as Code};
use super::extract::*;
use super::extract_plan::build_plan;
use super::progress::ProgressEvent;
use super::test_support::*;
use super::zip_fixture::*;
use crate::shared::cancel::CancelRegistry;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

struct Ctx {
    _tmp: tempfile::TempDir,
    work: PathBuf,
    dest: PathBuf,
}

fn ctx() -> Ctx {
    let tmp = tempdir();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    let dest = work.join("out");
    Ctx { _tmp: tmp, work, dest }
}

fn zip_of(c: &Ctx, builder: &ZipBuilder) -> PathBuf {
    builder.write_to(&c.work, "a.zip")
}

fn run(c: &Ctx, builder: &ZipBuilder) -> Result<ExtractResult, ArchiveError> {
    let zip = zip_of(c, builder);
    extract_simple(&zip, &c.dest, &c.work)
}

fn run_with(c: &Ctx, builder: &ZipBuilder, resolutions: &HashMap<String, Resolution>, hooks: &ExtractHooks) -> Result<ExtractResult, ArchiveError> {
    let zip = zip_of(c, builder);
    extract(&zip, &c.dest, &c.work, &env_plenty(), resolutions, hooks)
}

fn ok(result: Result<ExtractResult, ArchiveError>) -> ExtractResult {
    result.unwrap_or_else(|e| panic!("expected Ok(result), got Err({e})"))
}

fn completed(c: &Ctx, builder: &ZipBuilder) -> ExtractResult {
    let r = ok(run(c, builder));
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    r
}

fn res(pairs: &[(&str, Resolution)]) -> HashMap<String, Resolution> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

// ================================================================ basics

// AC-328 / AC-318 (extraction half) / AC-323: hierarchy, empty dirs, missing parents.
#[test]
fn ac328_entries_are_extracted_into_the_archive_hierarchy_creating_the_destination() {
    let c = ctx();
    let deep_dest = c.work.join("x").join("y").join("z");
    let zip = zip_of(&c, &ZipBuilder::new().dir("docs").file("docs/a.txt", b"hello").file("docs/sub/b.txt", b"world").dir("docs/empty").file("top.txt", b"top"));
    let r = ok(extract_simple(&zip, &deep_dest, &c.work));
    assert_eq!(r.outcome, ExtractOutcome::Completed);
    assert_eq!((r.extracted, r.skipped, r.total), (5, 0, 5));
    assert_eq!(read_string(&deep_dest.join("docs/a.txt")), "hello");
    assert_eq!(read_string(&deep_dest.join("docs/sub/b.txt")), "world");
    assert_eq!(read_string(&deep_dest.join("top.txt")), "top");
    assert!(deep_dest.join("docs/empty").is_dir(), "empty directories are preserved");
    assert_no_temp_leftovers(&deep_dest);
}

// AC-366.
#[test]
fn ac366_dot_prefixed_names_extract_to_the_plain_name() {
    let c = ctx();
    completed(&c, &ZipBuilder::new().file("./a.txt", b"x"));
    assert_eq!(read_string(&c.dest.join("a.txt")), "x");
}

#[test]
fn an_empty_archive_completes_with_nothing_extracted() {
    let c = ctx();
    let r = completed(&c, &ZipBuilder::new());
    assert_eq!((r.extracted, r.total), (0, 0));
}

// Interop: a genuinely deflate-compressed archive written by the zip crate.
#[test]
fn a_really_compressed_archive_written_by_the_zip_crate_extracts() {
    let c = ctx();
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 97) as u8).collect();
    let mut w = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    w.start_file("real/data.bin", opts).unwrap();
    w.write_all(&data).unwrap();
    w.add_directory("real/emptydir", zip::write::SimpleFileOptions::default()).unwrap();
    let bytes = w.finish().unwrap().into_inner();
    let zip = c.work.join("real.zip");
    fs::write(&zip, bytes).unwrap();
    let r = ok(extract_simple(&zip, &c.dest, &c.work));
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    assert_eq!(read_bytes(&c.dest.join("real/data.bin")), data);
    assert!(c.dest.join("real/emptydir").is_dir());
}

// R-346-1: ZIP64 structures extract like any archive.
#[test]
fn r346_1_a_zip64_structured_archive_extracts() {
    let c = ctx();
    let r = completed(&c, &ZipBuilder::new().entry(Entry::file("z.txt", b"zip64 data").zip64()).force_zip64_eocd());
    assert_eq!(r.extracted, 1);
    assert_eq!(read_string(&c.dest.join("z.txt")), "zip64 data");
}

// AC-376 / R-365-3: a ZIP inside a ZIP is just a file.
#[test]
fn ac376_a_nested_zip_is_not_extracted_recursively() {
    let c = ctx();
    let inner = ZipBuilder::new().file("deep.txt", b"deep").build();
    let outer = ZipBuilder::new().entry(Entry::stored("inner.zip", &inner));
    completed(&c, &outer);
    assert_eq!(read_bytes(&c.dest.join("inner.zip")), inner);
    assert!(!c.dest.join("deep.txt").exists() && !c.dest.join("inner").exists());
}

// AC-374-1: an empty entry with compressed size 0 becomes an empty file.
#[test]
fn ac374_1_a_zero_byte_deflate_entry_becomes_an_empty_file() {
    let c = ctx();
    completed(&c, &ZipBuilder::new().entry(Entry::declared("empty.txt", METHOD_DEFLATE, 0, 0)).file("real-empty.txt", b""));
    assert_eq!(fs::metadata(c.dest.join("empty.txt")).unwrap().len(), 0);
    assert_eq!(fs::metadata(c.dest.join("real-empty.txt")).unwrap().len(), 0);
}

// R-346-3: recorded attributes are NOT applied.
#[test]
fn r346_3_zip_attributes_are_not_applied() {
    let c = ctx();
    completed(
        &c,
        &ZipBuilder::new()
            .entry(Entry::file("readonly-hidden.txt", b"1").dos_attrs(0x01 | 0x02 | 0x04))
            .entry(Entry::file("mode444.txt", b"2").unix_mode(0o100444)),
    );
    for name in ["readonly-hidden.txt", "mode444.txt"] {
        let meta = fs::metadata(c.dest.join(name)).unwrap();
        assert!(!meta.permissions().readonly(), "{name} must not be read-only");
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            assert_eq!(meta.file_attributes() & (0x2 | 0x4), 0, "{name} must not be hidden/system");
        }
    }
}

// ================================================ rejection = nothing written

// R-360 / AC-365: whatever `build_plan` rejects, `extract` rejects with the SAME
// code and writes nothing -- also when the bad entry is last (both call the one
// validation implementation).
#[test]
fn ac365_extract_rejects_exactly_what_build_plan_rejects_and_writes_nothing() {
    let cases: Vec<(&str, ZipBuilder)> = vec![
        ("bad name last", ZipBuilder::new().file("ok1.txt", b"1").file("ok2.txt", b"2").file("../evil.txt", b"x")),
        ("encrypted", ZipBuilder::new().file("ok.txt", b"1").entry(Entry::file("e.txt", b"x").set_flag(FLAG_ENCRYPTED))),
        ("aes", ZipBuilder::new().file("ok.txt", b"1").entry(Entry::aes("a.txt", true))),
        ("bzip2", ZipBuilder::new().file("ok.txt", b"1").entry(Entry::file("b.txt", b"x").method(METHOD_BZIP2))),
        ("symlink entry", ZipBuilder::new().file("ok.txt", b"1").entry(Entry::symlink("l", "ok.txt"))),
        ("duplicate", ZipBuilder::new().file("a.txt", b"1").file("A.txt", b"2")),
        ("file and dir", ZipBuilder::new().file("a", b"1").file("a/b.txt", b"2")),
        ("multi disk", ZipBuilder::new().file("ok.txt", b"1").disks(1, 1)),
        ("ratio", ZipBuilder::new().file("ok.txt", b"1").entry(Entry::declared("bomb.bin", METHOD_DEFLATE, 1_032_001, 1000))),
    ];
    for (label, builder) in cases {
        let c = ctx();
        let zip = zip_of(&c, &builder);
        let expected = build_plan(&zip, &c.dest, &c.work, &env_plenty()).expect_err(label).code;
        let got = extract_simple(&zip, &c.dest, &c.work);
        assert_eq!(code_of(&got), Some(expected), "{label}");
        assert!(got.is_err(), "{label}: a validation failure is an `Err` (nothing was written)");
        assert!(!c.dest.exists(), "{label}: the destination must not have been created (R-360)");
    }
}

// AC-346-1 / R-345.
#[test]
fn ac346_1_a_bad_entry_wins_over_a_conflict_and_nothing_is_touched() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"existing");
    let before = tree(&c.dest);
    let got = run(&c, &ZipBuilder::new().file("a.txt", b"new").file("../evil.txt", b"x"));
    assert_eq!(code_of(&got), Some(Code::ZipEntryRejected));
    assert_eq!(tree(&c.dest), before);
}

// AC-324.
#[test]
fn ac324_a_destination_that_is_a_file_is_rejected() {
    let c = ctx();
    let file = write_file(&c.work, "occupied", b"i am a file");
    let zip = zip_of(&c, &ZipBuilder::new().file("a.txt", b"x"));
    let got = extract_simple(&zip, &file, &c.work);
    assert_eq!(code_of(&got), Some(Code::DestinationNotDir));
    assert_eq!(read_string(&file), "i am a file");
}

// ========================================================== conflicts

// AC-347 / R-375: an unresolved conflict rejects everything BEFORE writing.
#[test]
fn ac347_an_unresolved_conflict_is_rejected_before_anything_is_written() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"existing");
    let before = tree(&c.dest);
    let got = run(&c, &ZipBuilder::new().file("fresh.txt", b"new").file("a.txt", b"new"));
    assert_eq!(code_of(&got), Some(Code::Conflict));
    assert!(got.is_err(), "rejected up front, not a partial extraction");
    assert_eq!(tree(&c.dest), before, "no entry, not even the non-conflicting one, may be written");
}

// AC-343: overwrite -> replaced; the rest is extracted too.
#[test]
fn ac343_overwrite_replaces_conflicting_files_and_extracts_the_rest() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"old a");
    write_file(&c.dest, "sub/b.txt", b"old b");
    write_file(&c.dest, "untouched.txt", b"keep");
    let b = ZipBuilder::new().file("a.txt", b"new a").file("sub/b.txt", b"new b").file("c.txt", b"new c");
    let r = ok(run_with(&c, &b, &res(&[("a.txt", Resolution::Overwrite), ("sub/b.txt", Resolution::Overwrite)]), &ExtractHooks::none()));
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    assert_eq!((r.extracted, r.skipped, r.total), (3, 0, 3));
    assert_eq!(read_string(&c.dest.join("a.txt")), "new a");
    assert_eq!(read_string(&c.dest.join("sub/b.txt")), "new b");
    assert_eq!(read_string(&c.dest.join("c.txt")), "new c");
    assert_eq!(read_string(&c.dest.join("untouched.txt")), "keep");
    assert_no_temp_leftovers(&c.dest);
}

// AC-344: skip -> unchanged; only the rest is extracted; skipped is not "extracted".
#[test]
fn ac344_skip_leaves_conflicting_files_unchanged() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"old a");
    let b = ZipBuilder::new().file("a.txt", b"new a").file("c.txt", b"new c");
    let r = ok(run_with(&c, &b, &res(&[("a.txt", Resolution::Skip)]), &ExtractHooks::none()));
    assert_eq!(r.outcome, ExtractOutcome::Completed);
    assert_eq!((r.extracted, r.skipped, r.total), (1, 1, 2));
    assert_eq!(read_string(&c.dest.join("a.txt")), "old a");
    assert_eq!(read_string(&c.dest.join("c.txt")), "new c");
}

// AC-345: per-entry decisions.
#[test]
fn ac345_mixed_resolutions_are_applied_per_entry() {
    let c = ctx();
    write_file(&c.dest, "one.txt", b"old 1");
    write_file(&c.dest, "two.txt", b"old 2");
    let b = ZipBuilder::new().file("one.txt", b"new 1").file("two.txt", b"new 2");
    let r = ok(run_with(&c, &b, &res(&[("one.txt", Resolution::Overwrite), ("two.txt", Resolution::Skip)]), &ExtractHooks::none()));
    assert_eq!((r.extracted, r.skipped), (1, 1));
    assert_eq!(read_string(&c.dest.join("one.txt")), "new 1");
    assert_eq!(read_string(&c.dest.join("two.txt")), "old 2");
}

// One conflict resolved, another not -> still rejected up front (R-375).
#[test]
fn r375_every_conflict_needs_a_resolution() {
    let c = ctx();
    write_file(&c.dest, "one.txt", b"old 1");
    write_file(&c.dest, "two.txt", b"old 2");
    let b = ZipBuilder::new().file("one.txt", b"new 1").file("two.txt", b"new 2");
    let got = run_with(&c, &b, &res(&[("one.txt", Resolution::Overwrite)]), &ExtractHooks::none());
    assert_eq!(code_of(&got), Some(Code::Conflict));
    assert_eq!(read_string(&c.dest.join("one.txt")), "old 1", "nothing may be overwritten when the run is rejected");
}

// Resolutions for keys that are not conflicts are ignored (exact-key match).
#[test]
fn resolutions_for_unknown_or_non_conflicting_keys_are_ignored() {
    let c = ctx();
    let b = ZipBuilder::new().file("c.txt", b"new c");
    let r = ok(run_with(&c, &b, &res(&[("nope.txt", Resolution::Overwrite), ("C.TXT", Resolution::Skip), ("c.txt", Resolution::Skip)]), &ExtractHooks::none()));
    assert_eq!(r.outcome, ExtractOutcome::Completed);
    assert_eq!(read_string(&c.dest.join("c.txt")), "new c", "no conflict existed, so `Skip` must not suppress the entry");
}

// AC-349 / R-344: extracting into an existing tree that has directories only.
#[test]
fn ac349_existing_directories_are_merged_without_any_resolution() {
    let c = ctx();
    fs::create_dir_all(c.dest.join("docs")).unwrap();
    write_file(&c.dest, "docs/existing.txt", b"stay");
    completed(&c, &ZipBuilder::new().dir("docs").file("docs/new.txt", b"n"));
    assert_eq!(read_string(&c.dest.join("docs/existing.txt")), "stay");
    assert_eq!(read_string(&c.dest.join("docs/new.txt")), "n");
}

// MUST-66 / R-376 / AC-348: a same-named file appears AFTER the plan (planned as
// "no conflict") and BEFORE the commit. It must NOT be replaced; the run stops
// with `conflict`, the temp file is removed, earlier entries stay.
#[test]
fn must66_a_file_that_appears_before_the_commit_is_never_replaced() {
    let c = ctx();
    let intruder = |final_path: &Path| {
        if final_path.ends_with("race.txt") {
            fs::write(final_path, "intruder content").expect("create the racing file");
        }
    };
    let hooks = ExtractHooks { before_commit: &intruder, ..ExtractHooks::none() };
    let b = ZipBuilder::new().file("first.txt", b"first").file("race.txt", b"archive content").file("last.txt", b"last");
    let r = ok(run_with(&c, &b, &no_resolutions(), &hooks));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    assert_eq!(r.error.as_ref().map(|e| e.code), Some(Code::Conflict), "{:?}", r.error);
    assert_eq!(read_string(&c.dest.join("race.txt")), "intruder content", "the racing file must be untouched (MUST-66)");
    assert_eq!(read_string(&c.dest.join("first.txt")), "first", "entries before the failure stay (R-350)");
    assert!(!c.dest.join("last.txt").exists(), "the run stops at the first error");
    assert_eq!(r.extracted, 1);
    assert_no_temp_leftovers(&c.dest);
}

// MUST-66: same when the very FIRST entry races (nothing extracted yet).
#[test]
fn must66_a_racing_first_entry_is_a_failed_run_with_nothing_extracted() {
    let c = ctx();
    let intruder = |final_path: &Path| {
        let _ = fs::write(final_path, "intruder");
    };
    let hooks = ExtractHooks { before_commit: &intruder, ..ExtractHooks::none() };
    let got = run_with(&c, &ZipBuilder::new().file("only.txt", b"archive"), &no_resolutions(), &hooks);
    assert_eq!(code_of(&got), Some(Code::Conflict));
    assert_eq!(read_string(&c.dest.join("only.txt")), "intruder");
    if let Ok(r) = &got {
        assert_eq!(r.extracted, 0);
    }
    assert_no_temp_leftovers(&c.dest);
}

// AC-348 (spec's literal sequence): detect -> create the same name -> extract.
#[test]
fn ac348_a_file_created_after_detection_is_not_overwritten() {
    let c = ctx();
    let zip = zip_of(&c, &ZipBuilder::new().file("a.txt", b"archive"));
    let plan = build_plan(&zip, &c.dest, &c.work, &env_plenty()).unwrap();
    assert!(plan.conflicts.is_empty(), "no conflict at detection time");
    write_file(&c.dest, "a.txt", b"created by another process");
    let got = extract_simple(&zip, &c.dest, &c.work);
    assert_eq!(code_of(&got), Some(Code::Conflict));
    assert_eq!(read_string(&c.dest.join("a.txt")), "created by another process");
}

// The user chose "overwrite": the temp file replaces the existing one (and only then).
#[test]
fn an_explicit_overwrite_replaces_the_existing_file_through_the_temp_file() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"old");
    let b = ZipBuilder::new().file("a.txt", b"new");
    let r = ok(run_with(&c, &b, &res(&[("a.txt", Resolution::Overwrite)]), &ExtractHooks::none()));
    assert_eq!(r.outcome, ExtractOutcome::Completed);
    assert_eq!(read_string(&c.dest.join("a.txt")), "new");
    assert_no_temp_leftovers(&c.dest);
}

// R-348: while an entry is being committed its data lives in a fixed-shape temp
// file in the target directory, and the final name does not exist yet.
#[test]
fn r348_the_entry_is_written_to_a_temp_file_and_only_then_given_its_final_name() {
    let c = ctx();
    let seen: RefCell<Vec<(bool, Vec<String>)>> = RefCell::new(Vec::new());
    let probe = |final_path: &Path| {
        let dir = final_path.parent().unwrap();
        let names = tree_names(dir);
        seen.borrow_mut().push((final_path.exists(), names));
    };
    let hooks = ExtractHooks { before_commit: &probe, ..ExtractHooks::none() };
    ok(run_with(&c, &ZipBuilder::new().file("sub/data.txt", b"payload"), &no_resolutions(), &hooks));
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert!(!seen[0].0, "the final name must not exist before the commit");
    let temps: Vec<&String> = seen[0].1.iter().filter(|n| n.starts_with(".rfz-") && n.ends_with(".tmp")).collect();
    assert_eq!(temps.len(), 1, "exactly one fixed-shape temp file next to the target: {:?}", seen[0].1);
    assert!(!temps[0].contains("data"), "the temp name must not depend on the entry name (R-348-1)");
    assert_eq!(read_string(&c.dest.join("sub/data.txt")), "payload");
}

// AC-364-2 / R-348-1: a final path of exactly 259 units extracts fine.
#[test]
fn ac364_2_a_259_unit_path_extracts_without_the_temp_name_getting_in_the_way() {
    let c = ctx();
    let dest_len = plain_len(&c.dest);
    if dest_len + 2 > 259 || 259 - dest_len - 1 > 255 {
        skip("temp directory length outside the range this case can express");
        return;
    }
    let name = "n".repeat(259 - dest_len - 1);
    completed(&c, &ZipBuilder::new().file(&name, b"long"));
    assert_eq!(read_string(&c.dest.join(&name)), "long");
    assert_no_temp_leftovers(&c.dest);
}

fn plain_len(dest_after_creation: &Path) -> usize {
    // `dest` does not exist yet: measure the canonical parent + `\out`.
    let parent = fs::canonicalize(dest_after_creation.parent().unwrap()).unwrap();
    parent.join("out").to_string_lossy().trim_start_matches(r"\\?\").encode_utf16().count()
}

// ==================================================== R-350 failures / L-4 / CRC

// AC-373-1 / AC-330 / AC-332 / R-349-1: a bad CRC (data itself inflates fine)
// stops the run at that entry with `zip_corrupt`; earlier entries stay; the bad
// entry never gets its final name; nothing later is written; no temp remains.
#[test]
fn ac373_1_a_crc_mismatch_is_zip_corrupt_and_the_entry_never_appears() {
    let c = ctx();
    let b = ZipBuilder::new()
        .file("ok1.txt", b"first")
        .entry(Entry::stored("bad.txt", b"hello").crc(0xDEAD_BEEF))
        .file("after.txt", b"later");
    let r = ok(run(&c, &b));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    let e = r.error.as_ref().expect("error");
    assert_eq!(e.code, Code::ZipCorrupt);
    assert_eq!(e.entry_path.as_deref(), Some("bad.txt"), "R-351: the failing entry is reported");
    assert_eq!((r.extracted, r.total), (1, 3));
    assert_eq!(read_string(&c.dest.join("ok1.txt")), "first");
    assert!(!c.dest.join("bad.txt").exists(), "no half-written file under the final name");
    assert!(!c.dest.join("after.txt").exists());
    assert_no_temp_leftovers(&c.dest);
}

// The same for a DEFLATE entry (CRC verified after inflating).
#[test]
fn ac373_1_a_crc_mismatch_in_a_deflate_entry_is_detected_too() {
    let c = ctx();
    let b = ZipBuilder::new().entry(Entry::file("bad.txt", b"some deflated text").crc(0x1234_5678));
    let r = ok(run(&c, &b));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    assert_eq!(r.error.as_ref().map(|e| e.code), Some(Code::ZipCorrupt));
    assert!(!c.dest.join("bad.txt").exists());
}

// AC-331-1 / R-351-1: failing on the first entry -> `Ok(Failed)` with extracted == 0.
#[test]
fn ac331_1_a_failure_on_the_first_entry_extracts_nothing() {
    let c = ctx();
    let b = ZipBuilder::new().entry(Entry::stored("bad.txt", b"hello").crc(1)).file("later.txt", b"x");
    let r = ok(run(&c, &b));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    assert_eq!(r.extracted, 0);
    assert_eq!(r.total, 2);
    assert!(!c.dest.join("later.txt").exists());
}

// AC-332: the entry that fails while replacing an existing file leaves that file alone.
#[test]
fn ac332_a_failing_overwrite_leaves_the_existing_file_unchanged() {
    let c = ctx();
    write_file(&c.dest, "bad.txt", b"precious original");
    let b = ZipBuilder::new().entry(Entry::stored("bad.txt", b"hello").crc(0xBAD0_BAD0));
    let r = ok(run_with(&c, &b, &res(&[("bad.txt", Resolution::Overwrite)]), &ExtractHooks::none()));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    assert_eq!(read_string(&c.dest.join("bad.txt")), "precious original");
    assert_no_temp_leftovers(&c.dest);
}

// AC-373 / R-349 / L-4: the stream inflates to MORE than the declared size.
#[test]
fn ac373_data_larger_than_the_declared_size_is_zip_corrupt() {
    let c = ctx();
    let real = b"0123456789";
    let payload = deflate_stored(real);
    let liar = Entry::file("big.txt", real).declare_sizes(3, payload.len() as u64);
    let r = ok(run(&c, &ZipBuilder::new().file("ok.txt", b"1").entry(liar)));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    assert_eq!(r.error.as_ref().map(|e| e.code), Some(Code::ZipCorrupt));
    assert!(!c.dest.join("big.txt").exists());
    let oversized: Vec<String> = tree(&c.dest).into_iter().filter(|(_, v)| v.as_ref().map_or(false, |b| b.len() > 3)).map(|(k, _)| k).collect();
    assert!(oversized.is_empty(), "nothing above the declared size may be on disk: {oversized:?}");
    assert_no_temp_leftovers(&c.dest);
}

// R-349-1: fewer bytes than declared is corrupt as well.
#[test]
fn r349_1_data_smaller_than_the_declared_size_is_zip_corrupt() {
    let c = ctx();
    let real = b"abc";
    let payload = deflate_stored(real);
    let liar = Entry::file("short.txt", real).declare_sizes(10, payload.len() as u64);
    let r = ok(run(&c, &ZipBuilder::new().entry(liar)));
    assert_eq!(r.outcome, ExtractOutcome::Failed);
    assert_eq!(r.error.as_ref().map(|e| e.code), Some(Code::ZipCorrupt));
    assert!(!c.dest.join("short.txt").exists());
}

// ===================================================== copy_entry (L-4, cancel, disk full)

struct FailingWriter {
    kind: io::ErrorKind,
    accepted: usize,
}
impl Write for FailingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.accepted >= 4 {
            return Err(io::Error::from(self.kind));
        }
        let n = buf.len().min(4 - self.accepted);
        self.accepted += n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn never() -> bool {
    false
}

#[test]
fn copy_entry_copies_exactly_the_declared_size() {
    let mut out = Vec::new();
    let n = copy_entry(&mut io::Cursor::new(vec![7u8; 100_000]), &mut out, 100_000, &never).unwrap();
    assert_eq!(n, 100_000);
    assert_eq!(out.len(), 100_000);
}

// L-4: at most declared+1 bytes are ever read/written; more -> zip_corrupt.
#[test]
fn l4_copy_entry_stops_just_past_the_declared_size() {
    let mut out = Vec::new();
    let err = copy_entry(&mut io::Cursor::new(vec![1u8; 1_000_000]), &mut out, 10, &never).unwrap_err();
    assert_eq!(err.code, Code::ZipCorrupt);
    assert!(out.len() <= 11, "wrote {} bytes for a declared size of 10", out.len());
}

#[test]
fn copy_entry_reports_too_little_data_as_zip_corrupt() {
    let mut out = Vec::new();
    let err = copy_entry(&mut io::Cursor::new(vec![1u8; 5]), &mut out, 10, &never).unwrap_err();
    assert_eq!(err.code, Code::ZipCorrupt);
}

// A reader that fails with InvalidData is what the zip crate returns for a CRC mismatch.
#[test]
fn copy_entry_maps_a_crc_style_reader_error_to_zip_corrupt() {
    struct CrcFail(bool);
    impl Read for CrcFail {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.0 {
                self.0 = true;
                buf[..3].copy_from_slice(b"abc");
                return Ok(3);
            }
            Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid checksum"))
        }
    }
    let mut out = Vec::new();
    let err = copy_entry(&mut CrcFail(false), &mut out, 3, &never).unwrap_err();
    assert_eq!(err.code, Code::ZipCorrupt);
}

// AC-380-1 / R-395-2: a write failure such as a full disk is `io_error`.
#[test]
fn ac380_1_a_full_disk_while_writing_is_io_error() {
    let mut w = FailingWriter { kind: io::ErrorKind::StorageFull, accepted: 0 };
    let err = copy_entry(&mut io::Cursor::new(vec![1u8; 1000]), &mut w, 1000, &never).unwrap_err();
    assert_eq!(err.code, Code::IoError);
}

// R-395-3: permission problems are classified, not flattened into io_error.
#[test]
fn r395_3_a_permission_error_while_writing_is_permission_denied() {
    let mut w = FailingWriter { kind: io::ErrorKind::PermissionDenied, accepted: 0 };
    let err = copy_entry(&mut io::Cursor::new(vec![1u8; 1000]), &mut w, 1000, &never).unwrap_err();
    assert_eq!(err.code, Code::PermissionDenied);
}

// R-382 (recommended part, adopted by the design): cancellation is polled while
// copying, at least every 64 KiB, and stops the copy early.
#[test]
fn r382_copy_entry_polls_cancellation_every_64_kib_and_stops() {
    let polls = Cell::new(0u32);
    let cancel = || {
        polls.set(polls.get() + 1);
        polls.get() > 2
    };
    let mut out = Vec::new();
    let size = 4 << 20;
    let err = copy_entry(&mut io::Cursor::new(vec![1u8; size]), &mut out, size as u64, &cancel).unwrap_err();
    assert_eq!(err.code, Code::Canceled);
    assert!(out.len() < size, "the copy must stop early, wrote {} of {size}", out.len());
    assert!(out.len() <= 4 * 65_536, "a cancel must take effect within a few 64 KiB blocks, wrote {}", out.len());
}

// ================================================================ cancel

// AC-358-2 / R-387-2: canceled before any work -> nothing at all is written.
#[test]
fn ac358_2_cancel_requested_before_the_run_writes_nothing() {
    let c = ctx();
    let hooks = ExtractHooks { should_cancel: &|| true, ..ExtractHooks::none() };
    let got = run_with(&c, &ZipBuilder::new().file("a.txt", b"1").file("b.txt", b"2"), &no_resolutions(), &hooks);
    assert_eq!(code_of(&got), Some(Code::Canceled));
    assert!(!c.dest.exists(), "not even the destination directory may be created");
}

// R-387-2: a cancel that arrives while the plan is being built (i.e. after the
// first check, before writing starts) must not leave an empty destination behind.
#[test]
fn r387_2_a_cancel_during_planning_creates_no_destination_directory() {
    let c = ctx();
    let polls = Cell::new(0u32);
    let poll = || {
        polls.set(polls.get() + 1);
        polls.get() > 1
    };
    let hooks = ExtractHooks { should_cancel: &poll, ..ExtractHooks::none() };
    let got = run_with(&c, &ZipBuilder::new().file("a.txt", b"1"), &no_resolutions(), &hooks);
    assert_eq!(code_of(&got), Some(Code::Canceled));
    assert!(!c.dest.exists(), "the destination directory must not be created");
}

// AC-358-2 + shared registry: cancel first, register later -> the flag is already up.
#[test]
fn ac358_2_an_early_cancel_recorded_in_the_shared_registry_stops_the_run_before_it_writes() {
    let c = ctx();
    let registry = CancelRegistry::default();
    registry.request_cancel("op-early");
    let flag = registry.register("op-early");
    let poll = || flag.load(Ordering::Relaxed);
    let hooks = ExtractHooks { should_cancel: &poll, ..ExtractHooks::none() };
    let got = run_with(&c, &ZipBuilder::new().file("a.txt", b"1"), &no_resolutions(), &hooks);
    assert_eq!(code_of(&got), Some(Code::Canceled));
    assert!(!c.dest.exists());
    registry.unregister("op-early");
}

// AC-354 / AC-355 / AC-356 / R-384 / R-385: cancel raised while entry 3 of 5 starts.
#[test]
fn ac355_cancel_is_honoured_at_the_next_entry_boundary_and_extracted_entries_stay() {
    let c = ctx();
    let flag = AtomicBool::new(false);
    let raise = |ev: &ProgressEvent| {
        if ev.name == "e3.txt" {
            flag.store(true, Ordering::Relaxed);
        }
    };
    let poll = || flag.load(Ordering::Relaxed);
    let hooks = ExtractHooks { should_cancel: &poll, on_progress: &raise, ..ExtractHooks::none() };
    let mut b = ZipBuilder::new();
    for i in 1..=5 {
        b = b.file(&format!("e{i}.txt"), format!("entry {i}").as_bytes());
    }
    let r = ok(run_with(&c, &b, &no_resolutions(), &hooks));
    assert_eq!(r.outcome, ExtractOutcome::Canceled);
    assert_eq!(r.error.as_ref().map(|e| e.code), Some(Code::Canceled), "R-385: reported as `canceled`");
    assert_eq!(r.total, 5);
    assert!((2..=3).contains(&r.extracted), "entries before the request stay; at most the current one finishes: {}", r.extracted);
    assert_eq!(read_string(&c.dest.join("e1.txt")), "entry 1");
    assert_eq!(read_string(&c.dest.join("e2.txt")), "entry 2");
    assert!(!c.dest.join("e4.txt").exists() && !c.dest.join("e5.txt").exists(), "nothing after the boundary");
    assert_no_temp_leftovers(&c.dest);
}

// AC-355-1 (adopted): a cancel takes effect INSIDE a big entry; no partial file remains.
#[test]
fn ac355_1_cancel_inside_a_large_entry_leaves_no_partial_file() {
    let c = ctx();
    let polls = Cell::new(0u32);
    let cancel = || {
        polls.set(polls.get() + 1);
        polls.get() > 5
    };
    let hooks = ExtractHooks { should_cancel: &cancel, ..ExtractHooks::none() };
    let big = vec![7u8; 8 << 20];
    let r = ok(run_with(&c, &ZipBuilder::new().entry(Entry::stored("big.bin", &big)), &no_resolutions(), &hooks));
    assert_eq!(r.outcome, ExtractOutcome::Canceled);
    assert!(!c.dest.join("big.bin").exists(), "an interrupted entry must not keep its final name");
    assert_no_temp_leftovers(&c.dest);
    assert!(polls.get() <= 10, "the run must stop soon after the cancel ({} polls)", polls.get());
}

// R-381 / AC-350: progress is reported per entry, 1-based, with the archive path.
#[test]
fn r381_progress_is_reported_per_entry_with_index_total_and_name() {
    let c = ctx();
    let seen: RefCell<Vec<(String, usize, usize)>> = RefCell::new(Vec::new());
    let record = |ev: &ProgressEvent| seen.borrow_mut().push((ev.name.to_string(), ev.index, ev.total));
    let hooks = ExtractHooks { on_progress: &record, ..ExtractHooks::none() };
    ok(run_with(&c, &ZipBuilder::new().dir("d").file("d/a.txt", b"1").file("b.txt", b"2"), &no_resolutions(), &hooks));
    let seen = seen.borrow();
    assert_eq!(*seen, [("d".to_string(), 1, 3), ("d/a.txt".to_string(), 2, 3), ("b.txt".to_string(), 3, 3)]);
}

// ============================================== R-366 / R-367: links in the destination

// AC-368: a junction inside the destination that leads OUT of it.
#[test]
fn ac368_a_junction_that_escapes_the_destination_is_never_written_through() {
    let c = ctx();
    fs::create_dir_all(&c.dest).unwrap();
    let outside = c.work.join("outside");
    fs::create_dir(&outside).unwrap();
    if !make_dir_link(&c.dest.join("link"), &outside) {
        skip("cannot create a junction/symlink here");
        return;
    }
    let got = run(&c, &ZipBuilder::new().file("link/x.txt", b"escape").file("safe.txt", b"safe"));
    assert_eq!(code_of(&got), Some(Code::ZipEntryRejected));
    assert!(!outside.join("x.txt").exists(), "nothing may be written outside the destination");
    assert!(tree_names(&outside).is_empty());
    remove_dir_link(&c.dest.join("link"));
}

// A link that stays INSIDE the destination is fine (containment is judged after canonicalize).
#[test]
fn a_junction_that_stays_inside_the_destination_is_allowed() {
    let c = ctx();
    fs::create_dir_all(c.dest.join("real")).unwrap();
    if !make_dir_link(&c.dest.join("alias"), &c.dest.join("real")) {
        skip("cannot create a junction/symlink here");
        return;
    }
    let r = ok(run(&c, &ZipBuilder::new().file("alias/x.txt", b"inside")));
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    assert_eq!(read_string(&c.dest.join("real/x.txt")), "inside");
    remove_dir_link(&c.dest.join("alias"));
}

// R-366 (not cached): a directory that is verified for one entry is swapped for a
// junction to the outside before the next entry; the next entry must be re-checked.
#[test]
fn r366_containment_is_checked_per_entry_and_never_cached() {
    let c = ctx();
    let outside = c.work.join("outside");
    fs::create_dir(&outside).unwrap();
    let dest = c.dest.clone();
    let swapped = Cell::new(false);
    let outside_for_hook = outside.clone();
    let swap = |ev: &ProgressEvent| {
        if ev.name == "d/c.txt" && !swapped.get() {
            swapped.set(true);
            let d = dest.join("d");
            let _ = fs::remove_file(d.join("b.txt"));
            let _ = fs::remove_dir(&d);
            if !make_dir_link(&d, &outside_for_hook) {
                // could not build the attack: leave `d` missing and let the run proceed
                fs::create_dir_all(&d).unwrap();
            }
        }
    };
    let hooks = ExtractHooks { on_progress: &swap, ..ExtractHooks::none() };
    let b = ZipBuilder::new().file("d/b.txt", b"b").file("d/c.txt", b"c");
    let got = run_with(&c, &b, &no_resolutions(), &hooks);
    if !swapped.get() {
        skip("hook never ran");
        return;
    }
    let is_link = fs::symlink_metadata(c.dest.join("d")).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if !is_link {
        skip("cannot create a junction/symlink here");
        return;
    }
    assert_eq!(code_of(&got), Some(Code::ZipEntryRejected), "the swapped directory must be caught");
    assert!(!outside.join("c.txt").exists(), "nothing may be written through the swapped junction");
    remove_dir_link(&c.dest.join("d"));
}

// R-367 at run time: a link that becomes the final name after planning stops the run.
#[test]
fn r367_a_link_that_appears_at_the_final_name_after_planning_is_not_written_through() {
    let c = ctx();
    let outside = write_file(&c.work, "outside.txt", b"outside original");
    let plant = |final_path: &Path| {
        if final_path.ends_with("f.txt") {
            let _ = make_file_symlink(final_path, &outside);
        }
    };
    let hooks = ExtractHooks { before_commit: &plant, ..ExtractHooks::none() };
    let got = run_with(&c, &ZipBuilder::new().file("f.txt", b"hijack"), &no_resolutions(), &hooks);
    let planted = fs::symlink_metadata(c.dest.join("f.txt")).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if !planted {
        skip("file symlinks need the symlink privilege on Windows");
        return;
    }
    assert!(code_of(&got).is_some(), "the run must not report success");
    assert_eq!(read_string(&outside), "outside original", "the link target must be untouched");
}

// R-367-1 (run level): the archive itself is never overwritten.
#[test]
fn r367_1_an_entry_named_like_the_archive_does_not_overwrite_it() {
    let c = ctx();
    let builder = ZipBuilder::new().file("readme.txt", b"1").file("a.zip", b"overwrite me");
    let zip = zip_of(&c, &builder);
    let before = read_bytes(&zip);
    let got = extract_simple(&zip, &c.work, &c.work);
    assert_eq!(code_of(&got), Some(Code::ZipEntryRejected));
    assert_eq!(read_bytes(&zip), before);
    assert!(!c.work.join("readme.txt").exists(), "rejected up front: nothing written");
}

// MUST-65 at run level: extracting next to the archive works.
#[test]
fn must65_extracting_into_the_archives_own_folder_works() {
    let c = ctx();
    let zip = zip_of(&c, &ZipBuilder::new().file("readme.txt", b"1").dir("sub").file("sub/x.txt", b"2"));
    let r = ok(extract_simple(&zip, &c.work, &c.work));
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    assert_eq!(read_string(&c.work.join("sub/x.txt")), "2");
    assert!(zip.exists());
}

// ============================================================ R-361-1

// AC-365-1: whatever happens, only the CENTRAL directory name is used for writing.
#[test]
fn ac365_1_the_local_header_name_is_never_used_for_the_destination() {
    let c = ctx();
    let b = ZipBuilder::new().entry(Entry::file("ok.txt", b"content").local_name(b"../evil.txt"));
    let got = run(&c, &b);
    assert!(!c.work.join("evil.txt").exists(), "nothing may be written outside the destination");
    assert!(!c.dest.join("evil.txt").exists());
    match got {
        Ok(r) if r.outcome == ExtractOutcome::Completed => assert_eq!(read_string(&c.dest.join("ok.txt")), "content"),
        other => assert_eq!(code_of(&other), Some(Code::ZipCorrupt), "a mismatch detected by the zip crate is acceptable"),
    }
}

// ======================================================== R-346-2 modification times

const SECS_2020_01_02: std::ops::Range<u64> = 1_577_750_400..1_578_268_800;
const SECS_2019_05_06: std::ops::Range<u64> = 1_557_040_000..1_557_216_000;

fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

// R-346-2 / AC-319-1 (extraction half): files after the commit, NEW directories
// after all their children (so writing into them does not change the date).
#[test]
fn r346_2_file_and_new_directory_modification_times_are_restored() {
    let c = ctx();
    let b = ZipBuilder::new()
        .entry(Entry::dir("d").dos_datetime(dos_date(2019, 5, 6), dos_time(7, 8, 10)))
        .entry(Entry::file("d/f.txt", b"x").dos_datetime(dos_date(2020, 1, 2), dos_time(3, 4, 6)));
    completed(&c, &b);
    assert!(SECS_2020_01_02.contains(&unix_secs(mtime(&c.dest.join("d/f.txt")))), "file mtime restored");
    assert!(
        SECS_2019_05_06.contains(&unix_secs(mtime(&c.dest.join("d")))),
        "the directory date must be set AFTER its children were written"
    );
}

// R-346-2: the order of set_modified calls (child before its new parent), and a
// directory that existed before the run is not touched.
#[test]
fn r346_2_new_directories_are_stamped_after_their_children_and_existing_ones_not_at_all() {
    let c = ctx();
    fs::create_dir_all(c.dest.join("pre")).unwrap();
    let calls: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    let record = |p: &Path, _t: SystemTime| -> io::Result<()> {
        calls.borrow_mut().push(p.to_path_buf());
        Ok(())
    };
    let hooks = ExtractHooks { set_modified: &record, ..ExtractHooks::none() };
    let stamp = |name: &str| Entry::file(name, b"x").dos_datetime(dos_date(2020, 1, 2), dos_time(3, 4, 6));
    let dir_stamp = |name: &str| Entry::dir(name).dos_datetime(dos_date(2019, 5, 6), dos_time(7, 8, 10));
    let b = ZipBuilder::new().entry(dir_stamp("new")).entry(stamp("new/f.txt")).entry(dir_stamp("pre")).entry(stamp("pre/g.txt"));
    ok(run_with(&c, &b, &no_resolutions(), &hooks));
    let calls = calls.borrow();
    let pos = |suffix: &str| calls.iter().position(|p| p.ends_with(suffix));
    let f = pos("new/f.txt").or_else(|| pos(r"new\f.txt")).expect("file stamped");
    let d = calls.iter().position(|p| p.file_name().map_or(false, |n| n == "new")).expect("new directory stamped");
    assert!(f < d, "the child must be stamped before its new parent directory: {calls:?}");
    assert!(pos("pre/g.txt").or_else(|| pos(r"pre\g.txt")).is_some(), "files in an existing directory are stamped");
    assert!(!calls.iter().any(|p| p.file_name().map_or(false, |n| n == "pre")), "an existing directory must not be re-dated: {calls:?}");
}

// R-346-2: no / invalid date -> no restore attempt.
#[test]
fn r346_2_entries_without_a_valid_date_are_not_restored() {
    let c = ctx();
    let calls: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    let record = |p: &Path, _t: SystemTime| -> io::Result<()> {
        calls.borrow_mut().push(p.to_path_buf());
        Ok(())
    };
    let hooks = ExtractHooks { set_modified: &record, ..ExtractHooks::none() };
    let b = ZipBuilder::new().entry(Entry::file("undated.txt", b"x").dos_datetime(0, 0));
    ok(run_with(&c, &b, &no_resolutions(), &hooks));
    assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
    let age = secs_apart(SystemTime::now(), mtime(&c.dest.join("undated.txt")));
    assert!(age < 3600, "an undated entry keeps the time of extraction");
}

// AC-319-1 (last sentence) / R-346-2: a failing timestamp update never fails the run.
#[test]
fn ac319_1_a_failing_timestamp_update_does_not_fail_the_extraction() {
    let c = ctx();
    let fail = |_p: &Path, _t: SystemTime| -> io::Result<()> { Err(io::Error::from(io::ErrorKind::PermissionDenied)) };
    let hooks = ExtractHooks { set_modified: &fail, ..ExtractHooks::none() };
    let b = ZipBuilder::new().dir("d").file("d/f.txt", b"x").file("g.txt", b"y");
    let r = ok(run_with(&c, &b, &no_resolutions(), &hooks));
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    assert_eq!(r.extracted, 3);
    assert_eq!(read_string(&c.dest.join("d/f.txt")), "x");
}
