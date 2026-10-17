//! TDD (unit-test-writer, 2026-09-27): `archive::extract_plan::build_plan` — the
//! single validation implementation behind conflict detection and extraction.
//!
//! Specs (SPEC-0300-0014): R-345, R-360, R-361-1, R-362 #1-#11, R-363, R-364 /
//! R-364-1 / R-364-2, R-365 L-1..L-5 (+R-365-1/-4/-6), R-367, R-367-1, R-371,
//! R-372 / R-372-1 / R-372-2, R-343-1 / R-343-2 / R-341-1, R-346-1, R-346-2;
//! AC-324, AC-325, AC-327-1, AC-328-1, AC-346-1, AC-349, AC-349-2, AC-349-3,
//! AC-360, AC-360-1, AC-362, AC-362-1, AC-362-2, AC-363, AC-364, AC-364-1,
//! AC-364-2, AC-364-3, AC-365-1, AC-366, AC-367, AC-369, AC-369-1, AC-370 ..
//! AC-375, AC-374-1, AC-378, AC-378-1, AC-378-2.
//! SPEC-0200-0001 §3.4.4, §3.4.13-1 (MUST-65), §3.4.13-6 (MUST-70/71).
//!
//! `build_plan` never writes: every "rejected" test also asserts that the
//! destination directory was not created (R-360).

use super::error::{ArchiveError, ArchiveErrorCode as Code};
use super::extract_plan::*;
use super::test_support::*;
use super::zip_fixture::*;
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

struct Ctx {
    _tmp: tempfile::TempDir,
    /// Base directory (`base_dir`) and the directory holding the ZIPs.
    work: PathBuf,
    /// Default destination: does not exist yet.
    dest: PathBuf,
}

fn ctx() -> Ctx {
    let tmp = tempdir();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    let dest = work.join("out");
    Ctx { _tmp: tmp, work, dest }
}

fn plan_of(c: &Ctx, builder: &ZipBuilder) -> Result<Plan, ArchiveError> {
    let zip = builder.write_to(&c.work, "a.zip");
    build_plan(&zip, &c.dest, &c.work, &env_plenty())
}

fn plan_ok(c: &Ctx, builder: &ZipBuilder) -> Plan {
    plan_of(c, builder).unwrap_or_else(|e| panic!("expected an acceptable archive, got {e}"))
}

/// The error code of a rejected archive; also asserts nothing was written.
fn reject_code(c: &Ctx, builder: &ZipBuilder) -> Code {
    let err = plan_of(c, builder).expect_err("expected the archive to be rejected");
    assert!(!c.dest.exists(), "a rejected plan must not create the destination (R-360)");
    err.code
}

fn with_bad(name: &str) -> ZipBuilder {
    // A good entry first and last, the bad one in the middle.
    ZipBuilder::new().file("ok1.txt", b"1").entry(Entry::file(name, b"x")).file("ok2.txt", b"2")
}

fn plain(p: &Path) -> String {
    p.to_string_lossy().trim_start_matches(r"\\?\").to_string()
}

fn utf16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Canonical (verbatim on Windows) form, as `normalize_allowing_missing` yields.
fn canon(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap()
}

// ============================================================ happy path

#[test]
fn a_plain_archive_yields_entries_targets_and_totals_without_writing() {
    let c = ctx();
    let plan = plan_ok(
        &c,
        &ZipBuilder::new().dir("docs").file("docs/a.txt", b"hello").file("b.txt", b"world!").dir("docs/empty"),
    );
    assert_eq!(plan.total_entries, 4);
    assert_eq!(plan.entries.len(), 4);
    let paths: Vec<&str> = plan.entries.iter().map(|e| e.entry_path.as_str()).collect();
    assert_eq!(paths, ["docs", "docs/a.txt", "b.txt", "docs/empty"]);
    assert!(plan.entries[0].is_dir && !plan.entries[1].is_dir);
    assert_eq!(plan.total_uncompressed, 5 + 6);
    assert_eq!(plan.entries[1].uncompressed_size, 5);
    assert!(plan.conflicts.is_empty());
    assert_eq!(plan.destination, canon(&c.work).join("out"));
    assert_eq!(plan.entries[1].target, canon(&c.work).join("out").join("docs").join("a.txt"));
    assert!(!c.dest.exists(), "build_plan must not create the destination (R-360, R-371)");
}

// AC-366: `./a.txt` is accepted and becomes `a.txt`.
#[test]
fn ac366_dot_components_are_dropped_from_the_target() {
    let c = ctx();
    let plan = plan_ok(&c, &ZipBuilder::new().file("./a.txt", b"x").file("d/./e.txt", b"y"));
    assert_eq!(plan.entries[0].entry_path, "a.txt");
    assert_eq!(plan.entries[1].entry_path, "d/e.txt");
    assert_eq!(plan.entries[0].target, canon(&c.work).join("out").join("a.txt"));
}

// R-346-1: an archive that merely uses ZIP64 structures is accepted.
#[test]
fn r346_1_zip64_archives_are_accepted() {
    let c = ctx();
    let plan = plan_ok(&c, &ZipBuilder::new().entry(Entry::file("z.txt", b"data").zip64()).force_zip64_eocd());
    assert_eq!(plan.total_entries, 1);
}

// AC-328-1: 70,000 entries (headers only, ZIP64 EOCD) are not rejected.
#[test]
fn ac328_1_seventy_thousand_entries_are_accepted() {
    let c = ctx();
    let mut b = ZipBuilder::new();
    for i in 0..70_000 {
        b = b.entry(Entry::stored(&format!("f{i}.txt"), b""));
    }
    let plan = plan_ok(&c, &b);
    assert_eq!(plan.total_entries, 70_000);
}

// R-346-2: entry timestamps are carried; an invalid DOS date is `None`.
#[test]
fn r346_2_valid_dos_datetimes_are_carried_and_invalid_ones_are_none() {
    let c = ctx();
    let plan = plan_ok(
        &c,
        &ZipBuilder::new()
            .entry(Entry::file("dated.txt", b"x").dos_datetime(dos_date(2020, 1, 2), dos_time(3, 4, 6)))
            .entry(Entry::file("undated.txt", b"y").dos_datetime(0, 0)),
    );
    let modified = plan.entries[0].modified.expect("a valid date is kept");
    let secs = modified.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert!((1_577_750_400..1_578_268_800).contains(&secs), "around 2020-01-02 (any timezone): {secs}");
    assert!(plan.entries[1].modified.is_none(), "date 0 is invalid and must not be restored");
}

// ================================================================ inputs

#[test]
fn a_missing_zip_is_not_found_and_a_directory_is_not_a_zip_file() {
    let c = ctx();
    let err = build_plan(&c.work.join("missing.zip"), &c.dest, &c.work, &env_plenty()).unwrap_err();
    assert_eq!(err.code, Code::NotFound);
    let dir = c.work.join("dir.zip");
    fs::create_dir(&dir).unwrap();
    assert_eq!(build_plan(&dir, &c.dest, &c.work, &env_plenty()).unwrap_err().code, Code::NotFound);
}

// AC-324 / R-343-1.
#[test]
fn ac324_a_destination_that_is_an_existing_file_is_destination_not_dir() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.txt", b"x").write_to(&c.work, "a.zip");
    let file_dest = write_file(&c.work, "occupied", b"i am a file");
    let err = build_plan(&zip, &file_dest, &c.work, &env_plenty()).unwrap_err();
    assert_eq!(err.code, Code::DestinationNotDir);
    assert_eq!(read_string(&file_dest), "i am a file");
}

// AC-327-1 / R-341-1: a relative destination is resolved against `base_dir`.
#[test]
fn ac327_1_a_relative_destination_is_resolved_against_base_dir() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.txt", b"x").write_to(&c.work, "a.zip");
    let plan = build_plan(&zip, Path::new("sub"), &c.work, &env_plenty()).unwrap();
    assert_eq!(plan.destination, canon(&c.work).join("sub"));
    assert!(!c.work.join("sub").exists());
}

// R-341-1: drive-relative / root-relative destinations are ambiguous.
#[cfg(windows)]
#[test]
fn r341_1_drive_relative_and_root_relative_destinations_are_invalid_path() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.txt", b"x").write_to(&c.work, "a.zip");
    for dest in ["C:x", r"\x"] {
        let err = build_plan(&zip, Path::new(dest), &c.work, &env_plenty()).unwrap_err();
        assert_eq!(err.code, Code::InvalidPath, "{dest}");
    }
}

// AC-327-1 / R-343-2: `..` in the missing tail -> error, nothing created.
#[test]
fn ac327_1_a_destination_with_parent_components_in_the_missing_tail_is_rejected() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.txt", b"x").write_to(&c.work, "a.zip");
    let evil = c.work.join("newdir").join("..").join("..").join("x");
    let err = build_plan(&zip, &evil, &c.work, &env_plenty()).expect_err("`..` in the missing tail");
    assert!(matches!(err.code, Code::ZipEntryRejected | Code::InvalidPath), "{:?}", err.code);
    assert!(!c.work.join("newdir").exists() && !c.work.join("x").exists());
}

// R-343-2: the missing tail of the destination gets the R-363 checks too.
#[cfg(windows)]
#[test]
fn r343_2_the_missing_destination_tail_is_checked_with_r363() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.txt", b"x").write_to(&c.work, "a.zip");
    for tail in ["CON", "bad?name", "trailing.", "aux.txt"] {
        let dest = c.work.join(tail);
        let err = build_plan(&zip, &dest, &c.work, &env_plenty()).expect_err(tail);
        assert!(matches!(err.code, Code::ZipEntryRejected | Code::InvalidPath), "{tail}: {:?}", err.code);
    }
}

// ============================================== R-362 #1 / #4 / #3 / #2 / #6

// AC-362-1.
#[test]
fn ac362_1_a_file_that_is_not_a_zip_is_zip_corrupt() {
    let c = ctx();
    for (name, bytes) in [("text.zip", &b"this is not a zip file at all\n"[..]), ("empty.zip", &b""[..])] {
        let path = write_file(&c.work, name, bytes);
        let err = build_plan(&path, &c.dest, &c.work, &env_plenty()).unwrap_err();
        assert_eq!(err.code, Code::ZipCorrupt, "{name}");
    }
    assert!(!c.dest.exists());
}

// MUST-70: the declared count must match the real central directory.
#[test]
fn must70_an_eocd_count_that_disagrees_with_the_central_directory_is_zip_corrupt() {
    let c = ctx();
    let b = ZipBuilder::new().file("a.txt", b"a").file("b.txt", b"b").declare_entry_count(3).declare_entries_on_this_disk(3);
    assert_eq!(reject_code(&c, &b), Code::ZipCorrupt);
}

// AC-362-2.
#[test]
fn ac362_2_multi_volume_archives_are_zip_unsupported() {
    let c = ctx();
    assert_eq!(reject_code(&c, &ZipBuilder::new().file("a.txt", b"a").disks(1, 1)), Code::ZipUnsupported);
}

// AC-360 / AC-360-1: ZipCrypto flag and AES (method 99, with or without bit 0).
#[test]
fn ac360_encrypted_entries_are_zip_encrypted_not_unsupported() {
    let c = ctx();
    let zipcrypto = ZipBuilder::new().file("ok.txt", b"1").entry(Entry::file("enc.txt", b"x").set_flag(FLAG_ENCRYPTED));
    assert_eq!(reject_code(&c, &zipcrypto), Code::ZipEncrypted);
    let aes = ZipBuilder::new().file("ok.txt", b"1").entry(Entry::aes("aes.txt", true));
    assert_eq!(reject_code(&c, &aes), Code::ZipEncrypted, "AC-360-1");
    let aes_no_bit0 = ZipBuilder::new().file("ok.txt", b"1").entry(Entry::aes("aes2.txt", false));
    assert_eq!(reject_code(&c, &aes_no_bit0), Code::ZipEncrypted, "method 99 alone must be enough");
}

// AC-362 / R-392.
#[test]
fn ac362_methods_other_than_stored_and_deflate_are_zip_unsupported() {
    let c = ctx();
    for method in [METHOD_BZIP2, METHOD_LZMA, 9, 93, 98] {
        let b = ZipBuilder::new().file("ok.txt", b"1").entry(Entry::file("x.bin", b"data").method(method));
        assert_eq!(reject_code(&c, &b), Code::ZipUnsupported, "method {method}");
    }
}

// AC-367 / R-362 #6: one symlink entry rejects the WHOLE archive.
#[test]
fn ac367_a_symlink_entry_rejects_the_whole_archive() {
    let c = ctx();
    let b = ZipBuilder::new().file("ok.txt", b"1").entry(Entry::symlink("link.txt", "ok.txt")).file("ok2.txt", b"2");
    assert_eq!(reject_code(&c, &b), Code::ZipEntryRejected);
    let dir_link = ZipBuilder::new().entry(Entry::symlink("dirlink", "../elsewhere"));
    assert_eq!(reject_code(&c, &dir_link), Code::ZipEntryRejected);
}

// Guard against over-blocking: Unix regular files / directories are fine.
#[test]
fn unix_regular_files_and_directories_are_not_treated_as_symlinks() {
    let c = ctx();
    let plan = plan_ok(
        &c,
        &ZipBuilder::new()
            .entry(Entry::file("run.sh", b"#!/bin/sh").unix_mode(0o100755))
            .entry(Entry::dir("d").unix_mode(0o040755)),
    );
    assert_eq!(plan.total_entries, 2);
}

// ====================================================== R-363 (AC-363/364)

// AC-363 (10 cases) + AC-364 (14 cases) + AC-364-1: each rejected, nothing written.
#[test]
fn ac363_ac364_bad_names_are_rejected_with_zip_entry_rejected() {
    let c = ctx();
    let names = [
        // AC-363
        "../evil.txt", "a/../../evil.txt", r"a\..\..\evil.txt", "/abs.txt", r"\\server\share\x.txt", "//server/share/x",
        r"\\?\C:\x", r"\\.\COM1", "C:/x.txt", "C:x.txt",
        // AC-364
        "CON", "nul.txt", "nul.tar.gz", "CON .txt", "dir/COM1.log", "LPT9", "CONIN$", "CONOUT$", "COM¹", "LPT³.txt",
        "a.txt:stream", "name.", "name ", "a?b.txt",
        // AC-364-1
        "", "/",
    ];
    for name in names {
        assert_eq!(reject_code(&c, &with_bad(name)), Code::ZipEntryRejected, "{name:?}");
    }
}

// AC-365: the bad entry is LAST -> still nothing accepted (whole-archive validation).
#[test]
fn ac365_a_bad_entry_at_the_end_rejects_everything() {
    let c = ctx();
    let b = ZipBuilder::new().file("ok1.txt", b"1").file("ok2.txt", b"2").file("../evil.txt", b"x");
    assert_eq!(reject_code(&c, &b), Code::ZipEntryRejected);
}

// AC-364-3 / R-363 #11: bit 11 + invalid UTF-8 (a lone surrogate) -> `invalid_path`.
#[test]
fn ac364_3_a_name_with_an_unpaired_surrogate_is_invalid_path() {
    let c = ctx();
    let lone_surrogate = Entry::file("placeholder", b"x").name_bytes(&[b'a', 0xED, 0xA0, 0x80, b'.', b't']).flags(FLAG_UTF8);
    let b = ZipBuilder::new().file("ok.txt", b"1").entry(lone_surrogate);
    assert_eq!(reject_code(&c, &b), Code::InvalidPath);
}

// AC-364-2 (a): a component of 256 units is rejected; exactly 255 is accepted.
#[test]
fn ac364_2_a_component_of_256_units_is_rejected_and_255_is_accepted() {
    let c = ctx();
    if utf16(&plain(&canon(&c.work).join("out"))) + 1 + 255 > 259 {
        skip("the temp directory path is too long for this case");
        return;
    }
    let ok = plan_ok(&c, &ZipBuilder::new().file(&"a".repeat(255), b"x"));
    assert_eq!(ok.entries.len(), 1);
    assert_eq!(reject_code(&c, &ZipBuilder::new().file(&"a".repeat(256), b"x")), Code::ZipEntryRejected);
}

// AC-364-2 (b): file path 259 accepted / 260 rejected (destination + name).
#[test]
fn ac364_2_file_paths_of_259_are_accepted_and_260_rejected() {
    let c = ctx();
    let dest_len = utf16(&plain(&canon(&c.work).join("out")));
    if dest_len + 1 + 1 > 259 || 259 - dest_len - 1 > 255 {
        skip("temp directory length outside the range this case can express");
        return;
    }
    let name_259 = "n".repeat(259 - dest_len - 1);
    let name_260 = "n".repeat(260 - dest_len - 1);
    if name_260.len() > 255 {
        skip("cannot build a 260-unit path with a legal component length here");
        return;
    }
    plan_ok(&c, &ZipBuilder::new().file(&name_259, b"x"));
    assert_eq!(reject_code(&c, &ZipBuilder::new().file(&name_260, b"x")), Code::ZipEntryRejected);
}

// AC-364-2 (c) + SHOULD-90: directory 247 accepted / 248 rejected, and the
// implied parent directory of a file is held to 247 as well.
#[test]
fn ac364_2_directory_paths_of_247_are_accepted_and_248_rejected_including_implied_parents() {
    let c = ctx();
    let dest_len = utf16(&plain(&canon(&c.work).join("out")));
    if dest_len + 2 > 247 || 247 - dest_len - 1 > 255 {
        skip("temp directory length outside the range this case can express");
        return;
    }
    let dir_247 = "d".repeat(247 - dest_len - 1);
    let dir_248 = "d".repeat(248 - dest_len - 1);
    plan_ok(&c, &ZipBuilder::new().dir(&dir_247));
    assert_eq!(reject_code(&c, &ZipBuilder::new().dir(&dir_248)), Code::ZipEntryRejected, "directory entry of 248");
    // The file itself is short (< 259) but its implicit parent has 248 units.
    let implicit = ZipBuilder::new().file(&format!("{dir_248}/a.txt"), b"x");
    assert_eq!(reject_code(&c, &implicit), Code::ZipEntryRejected, "implicit parent of 248");
    plan_ok(&c, &ZipBuilder::new().file(&format!("{dir_247}/a.txt"), b"x"));
}

// ===================================================== R-364 duplicates

// AC-378: case-only differences between FILES are rejected; between DIRECTORIES allowed.
#[test]
fn ac378_case_only_duplicates_of_files_are_rejected_but_directories_may_repeat() {
    let c = ctx();
    let files = ZipBuilder::new().file("A.txt", b"1").file("a.txt", b"2");
    assert_eq!(reject_code(&c, &files), Code::ZipEntryRejected);
    let nested = ZipBuilder::new().file("Dir/Sub/X.txt", b"1").file("dir/sub/x.TXT", b"2");
    assert_eq!(reject_code(&c, &nested), Code::ZipEntryRejected);
    plan_ok(&c, &ZipBuilder::new().dir("Dir").dir("dir").file("dir/x.txt", b"1"));
}

// AC-378-1: identical names.
#[test]
fn ac378_1_identical_file_names_are_rejected_and_identical_directories_are_allowed() {
    let c = ctx();
    let files = ZipBuilder::new().file("a.txt", b"1").file("a.txt", b"2");
    assert_eq!(reject_code(&c, &files), Code::ZipEntryRejected);
    plan_ok(&c, &ZipBuilder::new().dir("d").dir("d").file("d/f.txt", b"x"));
}

// AC-378-2: file vs directory of the same name, in either order and via a parent.
#[test]
fn ac378_2_a_file_and_a_directory_of_the_same_name_are_rejected_in_any_order() {
    let c = ctx();
    let cases = [
        ZipBuilder::new().file("a", b"1").file("a/b.txt", b"2"),
        ZipBuilder::new().file("a/b.txt", b"2").file("a", b"1"),
        ZipBuilder::new().file("a", b"1").dir("a"),
        ZipBuilder::new().dir("a").file("a", b"1"),
        ZipBuilder::new().file("A", b"1").file("a/b.txt", b"2"), // R-364: case-folded
        ZipBuilder::new().file("x/y", b"1").file("x/y/z.txt", b"2"),
    ];
    for (i, b) in cases.iter().enumerate() {
        assert_eq!(reject_code(&c, b), Code::ZipEntryRejected, "case {i}");
    }
}

// SHOULD-91: `.` / empty components do not hide a duplicate.
#[test]
fn should91_spelling_variants_do_not_hide_duplicates() {
    let c = ctx();
    for (a, b) in [("a/b.txt", "a//b.txt"), ("a.txt", "./a.txt"), ("a/b.txt", r"a\b.txt"), ("d/x", "d/./x")] {
        let zip = ZipBuilder::new().file(a, b"1").file(b, b"2");
        assert_eq!(reject_code(&c, &zip), Code::ZipEntryRejected, "{a:?} vs {b:?}");
    }
}

// ============================================================ limits

// AC-370 / L-1 on the DECLARED count (MUST-70): `zip_too_large`, nothing built.
#[test]
fn ac370_a_declared_count_above_100_000_is_zip_too_large() {
    let c = ctx();
    assert_eq!(reject_code(&c, &ZipBuilder::new().declare_entry_count(100_001)), Code::ZipTooLarge);
}

// AC-370: exactly 100,000 entries are accepted (headers only).
#[test]
fn ac370_exactly_100_000_entries_are_accepted() {
    let c = ctx();
    let mut b = ZipBuilder::new();
    for i in 0..100_000 {
        b = b.entry(Entry::stored(&format!("f{i}"), b""));
    }
    let plan = plan_ok(&c, &b);
    assert_eq!(plan.total_entries, 100_000);
}

const EIGHT_GIB: u64 = 8 << 30;

fn eight_gib_entry(name: &str, extra_bytes: u64) -> Entry {
    let unc = EIGHT_GIB + extra_bytes;
    Entry::declared(name, METHOD_DEFLATE, unc, unc.div_ceil(1032))
}

// AC-371 / L-2: the sum of declared sizes, exactly 16 GiB accepted, +1 byte rejected.
#[test]
fn ac371_the_declared_size_sum_is_limited_to_16_gib() {
    let c = ctx();
    let at_limit = ZipBuilder::new().entry(eight_gib_entry("a.bin", 0)).entry(eight_gib_entry("b.bin", 0));
    let plan = plan_ok(&c, &at_limit);
    assert_eq!(plan.total_uncompressed, 16 << 30);
    let over = ZipBuilder::new().entry(eight_gib_entry("a.bin", 0)).entry(eight_gib_entry("b.bin", 1));
    assert_eq!(reject_code(&c, &over), Code::ZipTooLarge);
}

// AC-375: a "duplicate entry" bomb (many entries whose declared sizes sum far
// past L-2) is rejected before anything is written.
#[test]
fn ac375_many_entries_with_huge_declared_sizes_are_rejected() {
    let c = ctx();
    let mut b = ZipBuilder::new();
    for i in 0..3 {
        b = b.entry(Entry::declared(&format!("bomb{i}.bin"), METHOD_DEFLATE, 6 << 30, (6u64 << 30).div_ceil(1032)));
    }
    assert_eq!(reject_code(&c, &b), Code::ZipTooLarge);
}

// L-2 overflow safety: declared sizes whose u64 sum overflows must not panic
// (debug builds trap arithmetic overflow) and must be an error.
#[test]
fn l2_a_declared_size_sum_that_overflows_u64_is_rejected_without_panicking() {
    let c = ctx();
    let half = (u64::MAX / 2) + 1;
    let b = ZipBuilder::new()
        .entry(Entry::stored("a.bin", b"").declare_sizes(half, half))
        .entry(Entry::stored("b.bin", b"").declare_sizes(half, half));
    let err = plan_of(&c, &b).expect_err("must be rejected");
    assert!(matches!(err.code, Code::ZipTooLarge | Code::ZipCorrupt), "{:?}", err.code);
}

// AC-372 / L-3 (R-365-1): declared total 30 bytes; free space 29 -> insufficient, 30 -> ok.
#[test]
fn ac372_free_space_below_the_declared_total_is_insufficient_space() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.bin", &[1u8; 10]).file("b.bin", &[2u8; 20]).write_to(&c.work, "a.zip");
    let too_small = |_: &Path| -> io::Result<u64> { Ok(29) };
    let err = build_plan(&zip, &c.dest, &c.work, &ExtractEnv { free_space: &too_small }).unwrap_err();
    assert_eq!(err.code, Code::InsufficientSpace);
    assert!(!c.dest.exists());
    let exact = |_: &Path| -> io::Result<u64> { Ok(30) };
    assert!(build_plan(&zip, &c.dest, &c.work, &ExtractEnv { free_space: &exact }).is_ok(), "free == declared is enough");
}

// L-3 asks about the nearest EXISTING ancestor of the (possibly missing) destination.
#[test]
fn l3_free_space_is_queried_on_the_nearest_existing_ancestor() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.bin", b"data").write_to(&c.work, "a.zip");
    let asked: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    let recorder = |p: &Path| -> io::Result<u64> {
        asked.borrow_mut().push(p.to_path_buf());
        Ok(u64::MAX)
    };
    let deep = c.dest.join("not").join("yet");
    build_plan(&zip, &deep, &c.work, &ExtractEnv { free_space: &recorder }).unwrap();
    let asked = asked.borrow();
    assert_eq!(asked.len(), 1, "queried once");
    assert!(asked[0].exists(), "the queried path must exist: {}", asked[0].display());
    assert_eq!(canon(&asked[0]), canon(&c.work), "the nearest existing ancestor of the destination");
}

// AC-372-1 / R-365-6: a failing free-space query SKIPS L-3.
#[test]
fn ac372_1_a_failing_free_space_query_skips_l3() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.bin", &[1u8; 10]).write_to(&c.work, "a.zip");
    let failing = |_: &Path| -> io::Result<u64> { Err(io::Error::other("volume information unavailable")) };
    let plan = build_plan(&zip, &c.dest, &c.work, &ExtractEnv { free_space: &failing }).expect("L-3 skipped, not insufficient_space");
    assert_eq!(plan.total_entries, 1);
}

// L-3 must not hide L-2: over the size limit is `zip_too_large` even with no free-space info.
#[test]
fn l2_is_still_enforced_when_the_free_space_query_fails() {
    let c = ctx();
    let zip = ZipBuilder::new().entry(eight_gib_entry("a.bin", 0)).entry(eight_gib_entry("b.bin", 1)).write_to(&c.work, "a.zip");
    let failing = |_: &Path| -> io::Result<u64> { Err(io::Error::other("unavailable")) };
    let err = build_plan(&zip, &c.dest, &c.work, &ExtractEnv { free_space: &failing }).unwrap_err();
    assert_eq!(err.code, Code::ZipTooLarge);
}

// AC-374 / L-5 (R-365-4): ratio exactly 1,032:1 accepted; above rejected.
#[test]
fn ac374_a_deflate_ratio_above_1032_is_zip_corrupt() {
    let c = ctx();
    plan_ok(&c, &ZipBuilder::new().entry(Entry::declared("edge.bin", METHOD_DEFLATE, 1_032_000, 1000)));
    let over = ZipBuilder::new().file("ok.txt", b"1").entry(Entry::declared("bomb.bin", METHOD_DEFLATE, 1_032_001, 1000));
    assert_eq!(reject_code(&c, &over), Code::ZipCorrupt);
}

// AC-374-1: compressed 0 / uncompressed 0 is an acceptable empty file; 0 / >=1 is corrupt.
#[test]
fn ac374_1_zero_compressed_size_rules() {
    let c = ctx();
    plan_ok(&c, &ZipBuilder::new().entry(Entry::declared("empty.txt", METHOD_DEFLATE, 0, 0)));
    let lie = ZipBuilder::new().entry(Entry::declared("lie.txt", METHOD_DEFLATE, 1, 0));
    assert_eq!(reject_code(&c, &lie), Code::ZipCorrupt);
}

// R-365-4: a Stored entry whose compressed and uncompressed sizes differ is corrupt.
#[test]
fn r365_4_stored_entries_with_mismatched_sizes_are_zip_corrupt() {
    let c = ctx();
    let bad = ZipBuilder::new().entry(Entry::stored("s.txt", b"hello").declare_sizes(6, 5));
    assert_eq!(reject_code(&c, &bad), Code::ZipCorrupt);
    plan_ok(&c, &ZipBuilder::new().entry(Entry::stored("s.txt", b"hello")));
}

// ============================================ R-345 / R-372: conflicts

// AC-346-1 / R-345: a validation failure wins over conflict reporting.
#[test]
fn ac346_1_validation_errors_take_precedence_over_conflicts() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"existing");
    let b = ZipBuilder::new().file("a.txt", b"new").file("../evil.txt", b"x");
    let err = plan_of(&c, &b).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected);
    assert_eq!(read_string(&c.dest.join("a.txt")), "existing");
}

// R-372: existing files are reported by archive path; nothing else is.
#[test]
fn r372_existing_files_are_reported_as_conflicts() {
    let c = ctx();
    write_file(&c.dest, "a.txt", b"existing");
    write_file(&c.dest, "sub/b.txt", b"existing too");
    let plan = plan_ok(&c, &ZipBuilder::new().file("a.txt", b"new").file("sub/b.txt", b"new").file("c.txt", b"new").file("sub/d.txt", b"new"));
    let mut conflicts = plan.conflicts.clone();
    conflicts.sort();
    assert_eq!(conflicts, ["a.txt", "sub/b.txt"]);
    assert_eq!(read_string(&c.dest.join("a.txt")), "existing", "detection must not modify anything");
}

// AC-325 / AC-349 / R-344 / R-374-3: an existing directory is NOT a conflict.
#[test]
fn ac349_an_existing_directory_with_the_same_name_is_not_a_conflict() {
    let c = ctx();
    fs::create_dir_all(c.dest.join("docs")).unwrap();
    let plan = plan_ok(&c, &ZipBuilder::new().dir("docs").file("docs/a.txt", b"x"));
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
}

// AC-325: a pre-existing destination with unrelated content is fine.
#[test]
fn ac325_an_existing_destination_without_matching_files_has_no_conflicts() {
    let c = ctx();
    write_file(&c.dest, "unrelated.txt", b"keep");
    let plan = plan_ok(&c, &ZipBuilder::new().file("a.txt", b"x"));
    assert!(plan.conflicts.is_empty());
}

// AC-349-2 / R-372-2: on a case-insensitive volume `A.TXT` exists for `a.txt`.
#[cfg(windows)]
#[test]
fn ac349_2_a_case_variant_of_an_existing_file_is_a_conflict_on_windows() {
    let c = ctx();
    write_file(&c.dest, "A.TXT", b"existing");
    let plan = plan_ok(&c, &ZipBuilder::new().file("a.txt", b"new"));
    assert_eq!(plan.conflicts, ["a.txt"]);
}

// R-372-1 (approved proposal): kind mismatches reject the whole extraction with `conflict`.
#[test]
fn r372_1_kind_mismatches_are_conflict_errors() {
    let c = ctx();
    fs::create_dir_all(c.dest.join("a.txt")).unwrap();
    assert_eq!(plan_of(&c, &ZipBuilder::new().file("a.txt", b"x")).unwrap_err().code, Code::Conflict, "file entry over a directory");
    fs::remove_dir(c.dest.join("a.txt")).unwrap();

    write_file(&c.dest, "d", b"i am a file");
    assert_eq!(plan_of(&c, &ZipBuilder::new().dir("d")).unwrap_err().code, Code::Conflict, "directory entry over a file");
    assert_eq!(plan_of(&c, &ZipBuilder::new().file("d/x.txt", b"x")).unwrap_err().code, Code::Conflict, "parent is a file");
    assert_eq!(read_string(&c.dest.join("d")), "i am a file");
}

// ================================================ R-367 / R-367-1 / R-366

// AC-349-3 / R-367: a final name that is a link with NO target -> reject everything
// (not a conflict, not an overwrite choice). Junction variant (needs no privilege).
#[test]
fn ac349_3_a_dangling_directory_link_at_the_final_name_rejects_the_whole_archive() {
    let c = ctx();
    fs::create_dir_all(&c.dest).unwrap();
    if !make_dangling_dir_link(&c.dest.join("f.txt"), &c.work.join("scratch")) {
        skip("cannot create a dangling junction/symlink here");
        return;
    }
    let err = plan_of(&c, &ZipBuilder::new().file("ok.txt", b"1").file("f.txt", b"x")).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected);
    assert!(!c.work.join("scratch").exists(), "nothing may be created through the link");
    remove_dir_link(&c.dest.join("f.txt"));
}

// AC-349-3 / AC-369 (file symlink variants; need the symlink privilege on Windows).
#[test]
fn ac369_a_file_symlink_at_the_final_name_is_zip_entry_rejected_and_its_target_untouched() {
    let c = ctx();
    fs::create_dir_all(&c.dest).unwrap();
    let outside = write_file(&c.work, "outside.txt", b"outside original");
    if !make_file_symlink(&c.dest.join("f.txt"), &outside) {
        skip("file symlinks need the symlink privilege on Windows");
        return;
    }
    let err = plan_of(&c, &ZipBuilder::new().file("f.txt", b"hijack")).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected, "a link is rejected, never offered as an overwrite conflict");
    assert_eq!(read_string(&outside), "outside original");

    // and with no target at all
    fs::remove_file(&outside).unwrap();
    let err = plan_of(&c, &ZipBuilder::new().file("f.txt", b"hijack")).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected, "a dangling link counts as existing (R-372-2)");
}

// R-367: a healthy junction sitting on a FILE entry's final name is also a link -> rejected.
#[test]
fn r367_a_directory_junction_at_a_file_entrys_final_name_is_rejected_as_a_link() {
    let c = ctx();
    fs::create_dir_all(&c.dest).unwrap();
    let real = c.work.join("real");
    fs::create_dir(&real).unwrap();
    if !make_dir_link(&c.dest.join("f.txt"), &real) {
        skip("cannot create a junction/symlink here");
        return;
    }
    let err = plan_of(&c, &ZipBuilder::new().file("f.txt", b"x")).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected);
    remove_dir_link(&c.dest.join("f.txt"));
}

// AC-369-1 / R-367-1: the archive itself must not be overwritten by an entry.
#[test]
fn ac369_1_an_entry_that_would_overwrite_the_archive_itself_is_rejected() {
    let c = ctx();
    for name in ["a.zip", "./a.zip"] {
        let zip = ZipBuilder::new().file("readme.txt", b"1").file(name, b"overwrite me").write_to(&c.work, "a.zip");
        let err = build_plan(&zip, &c.work, &c.work, &env_plenty()).unwrap_err();
        assert_eq!(err.code, Code::ZipEntryRejected, "{name}");
    }
}

#[cfg(windows)]
#[test]
fn ac369_1_a_letter_case_variant_of_the_archive_name_is_rejected_too() {
    let c = ctx();
    let zip = ZipBuilder::new().file("A.ZIP", b"overwrite me").write_to(&c.work, "a.zip");
    let err = build_plan(&zip, &c.work, &c.work, &env_plenty()).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected);
}

// R-367-1 / R-325-1: the archive reached through a junction alias is still "itself".
#[test]
fn ac369_1_the_archive_reached_through_a_junction_alias_is_recognised() {
    let c = ctx();
    let zip = ZipBuilder::new().file("a.zip", b"overwrite me").write_to(&c.work, "a.zip");
    let alias = c._tmp.path().join("alias");
    if !make_dir_link(&alias, &c.work) {
        skip("cannot create a junction/symlink here");
        return;
    }
    let err = build_plan(&alias.join("a.zip"), &c.work, &c.work, &env_plenty()).unwrap_err();
    assert_eq!(err.code, Code::ZipEntryRejected);
    let _ = zip;
    remove_dir_link(&alias);
}

// MUST-65 (SPEC-0200-0001 §3.4.13-1): extracting INTO the archive's own folder is normal.
// The final paths do not exist, so `same_entity` (which is `None` for a missing
// path) must never be consulted -- otherwise every such extraction is rejected.
#[test]
fn must65_extracting_next_to_the_archive_is_not_rejected_when_the_final_paths_do_not_exist() {
    let c = ctx();
    let zip = ZipBuilder::new().file("readme.txt", b"1").file("sub/x.txt", b"2").dir("emptydir").write_to(&c.work, "a.zip");
    let plan = build_plan(&zip, &c.work, &c.work, &env_plenty()).expect("must not be mistaken for a ZIP-self overwrite");
    assert_eq!(plan.total_entries, 3);
    assert!(plan.conflicts.is_empty());
    assert_eq!(read_bytes(&zip), ZipBuilder::new().file("readme.txt", b"1").file("sub/x.txt", b"2").dir("emptydir").build());
}

// MUST-65: an EXISTING, different file is a normal conflict, not "the ZIP".
#[test]
fn must65_an_existing_unrelated_file_is_a_conflict_not_a_zip_self_rejection() {
    let c = ctx();
    write_file(&c.work, "other.txt", b"existing");
    let zip = ZipBuilder::new().file("other.txt", b"new").write_to(&c.work, "a.zip");
    let plan = build_plan(&zip, &c.work, &c.work, &env_plenty()).expect("plan");
    assert_eq!(plan.conflicts, ["other.txt"]);
}

// =============================================== R-361-1: central name only

// AC-365-1: the CENTRAL directory name (`ok.txt`) is what is validated and planned;
// the forged local-header name (`../evil.txt`) is never used.
#[test]
fn ac365_1_the_local_header_name_is_ignored() {
    let c = ctx();
    let b = ZipBuilder::new().entry(Entry::file("ok.txt", b"x").local_name(b"../evil.txt"));
    match plan_of(&c, &b) {
        Ok(plan) => {
            assert_eq!(plan.entries.len(), 1);
            assert_eq!(plan.entries[0].entry_path, "ok.txt");
            assert_eq!(plan.entries[0].target, canon(&c.work).join("out").join("ok.txt"));
        }
        // The zip crate itself may notice the mismatch: acceptable, but it must be an error, not a pass.
        Err(e) => assert_eq!(e.code, Code::ZipCorrupt),
    }
}

// R-361-1: a bad CENTRAL name is rejected even if the local header looks innocent.
#[test]
fn r361_1_a_bad_central_name_is_rejected_even_when_the_local_name_is_innocent() {
    let c = ctx();
    let b = ZipBuilder::new().entry(Entry::file("../evil.txt", b"x").local_name(b"ok.txt"));
    let err = plan_of(&c, &b).unwrap_err();
    assert!(matches!(err.code, Code::ZipEntryRejected | Code::ZipCorrupt), "{:?}", err.code);
    assert!(!c.dest.exists());
}

// Defensive: the name the zip crate reports for an index must be the name our
// own central-directory scan recorded (guards against a crate update changing
// the index/merge behaviour).
#[test]
fn the_crate_name_must_equal_the_scanned_central_directory_name() {
    assert!(ensure_same_name(b"a/b.txt", b"a/b.txt").is_ok());
    let err = ensure_same_name(b"other.txt", b"a/b.txt").unwrap_err();
    assert_eq!(err.code, Code::ZipCorrupt);
    assert_eq!(err.entry_path.as_deref(), Some("a/b.txt"));
}
