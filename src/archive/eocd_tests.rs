//! TDD (unit-test-writer, 2026-09-27): `archive::eocd` — EOCD pre-check before
//! `ZipArchive::new` (multi-disk, L-1 on the DECLARED entry count).
//!
//! Specs: SPEC-0300-0014 R-362 #1 / #4, R-365 L-1, AC-362-1, AC-362-2, AC-370;
//! SPEC-0200-0001 §3.4.13-6 (MUST-70). The fixtures are EOCD-only archives
//! (`ZipBuilder::declare_entry_count`) so no 100,000-entry archive is built.

use super::eocd::{check_eocd, inspect, MAX_ENTRIES};
use super::error::ArchiveErrorCode;
use super::test_support::*;
use super::zip_fixture::*;
use std::io::Cursor;

fn check(bytes: Vec<u8>) -> Result<u64, ArchiveErrorCode> {
    check_eocd(&mut Cursor::new(bytes))
        .map(|i| i.total_entries)
        .map_err(|e| e.code)
}

#[test]
fn l1_limit_constant_is_100_000() {
    assert_eq!(MAX_ENTRIES, 100_000);
}

// A normal archive reports its entry count.
#[test]
fn eocd_reports_the_declared_entry_count() {
    let bytes = ZipBuilder::new()
        .file("a.txt", b"a")
        .file("b.txt", b"b")
        .dir("d")
        .build();
    assert_eq!(check(bytes), Ok(3));
}

#[test]
fn eocd_accepts_an_archive_with_no_entries() {
    assert_eq!(check(ZipBuilder::new().build()), Ok(0));
}

// The EOCD may be followed by a comment (backwards search).
#[test]
fn eocd_is_found_behind_a_trailing_comment() {
    let bytes = ZipBuilder::new()
        .file("a.txt", b"a")
        .comment(&vec![b'x'; 1000])
        .build();
    assert_eq!(check(bytes), Ok(1));
}

// AC-370: L-1 boundary on the DECLARED count (100,000 ok, 100,001 too large).
// 100,000 needs the ZIP64 record (0xFFFF sentinel), so this also covers reading it.
#[test]
fn ac370_declared_100_000_entries_is_accepted_and_100_001_is_too_large() {
    assert_eq!(
        check(ZipBuilder::new().declare_entry_count(100_000).build()),
        Ok(100_000)
    );
    assert_eq!(
        check(ZipBuilder::new().declare_entry_count(100_001).build()),
        Err(ArchiveErrorCode::ZipTooLarge)
    );
}

#[test]
fn l1_a_zip64_count_far_beyond_the_limit_is_too_large() {
    assert_eq!(
        check(ZipBuilder::new().declare_entry_count(5_000_000_000).build()),
        Err(ArchiveErrorCode::ZipTooLarge)
    );
    assert_eq!(
        check(ZipBuilder::new().declare_entry_count(u64::MAX).build()),
        Err(ArchiveErrorCode::ZipTooLarge)
    );
}

// R-346-1: counts around the 16-bit boundary are NOT rejected (ZIP64 as such is fine).
#[test]
fn r346_1_counts_around_65535_are_accepted() {
    for n in [65_534u64, 65_535, 65_536, 70_000] {
        assert_eq!(
            check(ZipBuilder::new().declare_entry_count(n).build()),
            Ok(n),
            "{n}"
        );
    }
}

// The ZIP64 record is read even when the archive is otherwise small.
#[test]
fn eocd_reads_the_zip64_record_when_present() {
    let bytes = ZipBuilder::new()
        .file("a.txt", b"a")
        .force_zip64_eocd()
        .build();
    assert_eq!(check(bytes), Ok(1));
}

// AC-362-2 / R-362 #4: multi-volume archives are unsupported.
#[test]
fn ac362_2_multi_disk_eocds_are_unsupported() {
    for (this_disk, cd_disk) in [(1u16, 1u16), (0, 1), (1, 0), (3, 3)] {
        let bytes = ZipBuilder::new()
            .file("a.txt", b"a")
            .disks(this_disk, cd_disk)
            .build();
        assert_eq!(
            check(bytes),
            Err(ArchiveErrorCode::ZipUnsupported),
            "disks ({this_disk}, {cd_disk})"
        );
    }
}

#[test]
fn ac362_2_entries_on_this_disk_differing_from_the_total_is_unsupported() {
    let bytes = ZipBuilder::new()
        .declare_entry_count(5)
        .declare_entries_on_this_disk(3)
        .build();
    assert_eq!(check(bytes), Err(ArchiveErrorCode::ZipUnsupported));
}

#[test]
fn ac362_2_zip64_multi_disk_fields_are_unsupported() {
    let by_total_disks = ZipBuilder::new()
        .file("a.txt", b"a")
        .force_zip64_eocd()
        .zip64_total_disks(2)
        .build();
    assert_eq!(check(by_total_disks), Err(ArchiveErrorCode::ZipUnsupported));
    let by_disk_number = ZipBuilder::new()
        .file("a.txt", b"a")
        .force_zip64_eocd()
        .disks(1, 1)
        .build();
    assert_eq!(check(by_disk_number), Err(ArchiveErrorCode::ZipUnsupported));
}

// AC-362-1 / R-362 #1: not a ZIP at all -> zip_corrupt.
#[test]
fn ac362_1_non_zip_content_is_zip_corrupt() {
    assert_eq!(
        check(Vec::new()),
        Err(ArchiveErrorCode::ZipCorrupt),
        "empty file"
    );
    assert_eq!(
        check(b"PK".to_vec()),
        Err(ArchiveErrorCode::ZipCorrupt),
        "shorter than an EOCD"
    );
    assert_eq!(
        check(vec![0xAA; 100]),
        Err(ArchiveErrorCode::ZipCorrupt),
        "garbage"
    );
    assert_eq!(
        check(b"just a text file\nwith two lines\n".to_vec()),
        Err(ArchiveErrorCode::ZipCorrupt)
    );
    assert_eq!(
        check(vec![0u8; 70_000]),
        Err(ArchiveErrorCode::ZipCorrupt),
        "larger than the EOCD search window"
    );
}

// The signature without a complete record after it is corrupt, not a panic.
#[test]
fn ac362_1_truncated_eocd_is_zip_corrupt() {
    let full = ZipBuilder::new().file("a.txt", b"a").build();
    let truncated = full[..full.len() - 8].to_vec();
    assert_eq!(check(truncated), Err(ArchiveErrorCode::ZipCorrupt));
}

// `inspect(path)`: the file wrapper.
#[test]
fn inspect_reads_a_file_and_reports_a_missing_file_as_not_found() {
    let tmp = tempdir();
    let path = ZipBuilder::new()
        .file("a.txt", b"a")
        .write_to(tmp.path(), "ok.zip");
    assert_eq!(
        inspect(&path).map(|i| i.total_entries).map_err(|e| e.code),
        Ok(1)
    );
    assert_eq!(
        inspect(&tmp.path().join("missing.zip"))
            .map(|_| ())
            .map_err(|e| e.code),
        Err(ArchiveErrorCode::NotFound)
    );
}

// Overwrites the 16-bit "entries" fields of the trailing EOCD (no comment).
fn patch_16bit_counts(mut bytes: Vec<u8>, count: u16) -> Vec<u8> {
    let eocd = bytes.len() - 22;
    bytes[eocd + 8..eocd + 10].copy_from_slice(&count.to_le_bytes());
    bytes[eocd + 10..eocd + 12].copy_from_slice(&count.to_le_bytes());
    bytes
}

// A 16-bit count that is not the 0xFFFF sentinel must agree with the ZIP64
// record; a disagreement is corrupt (never silently trust one of them).
#[test]
fn a_16bit_count_disagreeing_with_the_zip64_count_is_zip_corrupt() {
    let base = ZipBuilder::new()
        .file("a.txt", b"a")
        .force_zip64_eocd()
        .build();
    assert_eq!(
        check(patch_16bit_counts(base.clone(), 5)),
        Err(ArchiveErrorCode::ZipCorrupt)
    );
    assert_eq!(
        check(patch_16bit_counts(base, 1)),
        Ok(1),
        "agreeing counts are fine"
    );
}
