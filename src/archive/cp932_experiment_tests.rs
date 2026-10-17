//! CP932 / zip-crate behavior EXPERIMENT (ISS-0099; SPEC-0200-0001 §3.4.9-c
//! F1-F7, §3.4.13-6 F8-F10, §3.4.14 C3 / C4).
//!
//! These tests OBSERVE what the `zip` crate (7.2, features `deflate` + `time`)
//! does with hand-assembled archives and print the result (`cargo test --lib
//! cp932_experiment -- --nocapture`). They are deliberately loose: their
//! assertions only pin facts the design already relies on (`name_raw()` gives
//! the untouched bytes; a CRC mismatch is reported by the crate, AC-373-1 / C4).
//! Everything else is logged so that code-generator / the orchestrator can decide
//! branch 1 / 2 / 3 of §3.4.9-c and fix the expectations afterwards.
//!
//! ## Observed results (zip 7.2.0, recorded 2026-09-27 by unit-test-writer)
//! See the `OBSERVATIONS` block at the bottom of this file (filled in after the
//! first run).
//!
//! No production stub is used here; this file is expected GREEN.

use super::zip_fixture::*;
use std::io::{Cursor, Read};

#[derive(Debug)]
#[allow(dead_code)]
struct EntryObs {
    name: String,
    name_raw: Vec<u8>,
    method: String,
    encrypted: bool,
    is_symlink: bool,
    unix_mode: Option<u32>,
    is_dir: bool,
    size: u64,
    compressed_size: u64,
    last_modified: String,
    /// `by_index` + `read_to_end`: `Ok(bytes read)` or the error text.
    read: Result<usize, String>,
    /// Name reported through `by_index` (decompressing accessor).
    name_via_by_index: Result<String, String>,
}

#[derive(Debug)]
struct Obs {
    open: Result<usize, String>,
    entries: Vec<EntryObs>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn observe(label: &str, bytes: Vec<u8>) -> Obs {
    let mut archive = match zip::ZipArchive::new(Cursor::new(bytes)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[{label}] ZipArchive::new -> Err: {e}");
            return Obs { open: Err(e.to_string()), entries: Vec::new() };
        }
    };
    let len = archive.len();
    eprintln!("[{label}] ZipArchive::new -> Ok, len = {len}");
    let mut entries = Vec::new();
    for i in 0..len {
        let meta = match archive.by_index_raw(i) {
            Ok(f) => Some((
                f.name().to_string(),
                f.name_raw().to_vec(),
                format!("{:?}", f.compression()),
                f.encrypted(),
                f.is_symlink(),
                f.unix_mode(),
                f.is_dir(),
                f.size(),
                f.compressed_size(),
                format!("{:?}", f.last_modified()),
            )),
            Err(e) => {
                eprintln!("[{label}]   #{i} by_index_raw -> Err: {e}");
                None
            }
        };
        let Some((name, name_raw, method, encrypted, is_symlink, unix_mode, is_dir, size, compressed_size, last_modified)) = meta else {
            continue;
        };
        let (name_via_by_index, read) = match archive.by_index(i) {
            Ok(mut f) => {
                let n = f.name().to_string();
                let mut sink = Vec::new();
                let r = f.read_to_end(&mut sink).map_err(|e| format!("{:?}: {e}", e.kind()));
                (Ok(n), r)
            }
            Err(e) => (Err(e.to_string()), Err(format!("by_index failed: {e}"))),
        };
        eprintln!(
            "[{label}]   #{i} name() = {name:?}\n[{label}]        name_raw = [{}]\n[{label}]        method = {method}, encrypted = {encrypted}, is_symlink = {is_symlink}, unix_mode = {unix_mode:?}, is_dir = {is_dir}, size = {size}, compressed = {compressed_size}, mtime = {last_modified}\n[{label}]        by_index name = {name_via_by_index:?}, read = {read:?}",
            hex(&name_raw)
        );
        entries.push(EntryObs { name, name_raw, method, encrypted, is_symlink, unix_mode, is_dir, size, compressed_size, last_modified, read, name_via_by_index });
    }
    Obs { open: Ok(len), entries }
}

fn one(name_bytes: &[u8], flags: u16) -> Entry {
    // Entry::file("x", ..) then forge the raw name and flags.
    Entry::file("placeholder", b"payload").name_bytes(name_bytes).flags(flags)
}

// ---------------------------------------------------------------------------
// F1-F7 (SPEC-0200-0001 §3.4.9-c)
// ---------------------------------------------------------------------------

// F1: `テスト.txt` as CP932 bytes, bit 11 clear.
#[test]
fn cp932_experiment_f1_cp932_name_without_the_utf8_flag() {
    let name = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'.', b't', b'x', b't'];
    let o = observe("F1 CP932 テスト.txt, no bit 11", ZipBuilder::new().entry(one(&name, 0)).build());
    assert_eq!(o.open, Ok(1));
    assert_eq!(o.entries[0].name_raw, name, "name_raw() must return the untouched bytes (branch 1 prerequisite)");
}

// F2: `ソース/表.txt`: 0x5C is the 2nd byte of ソ and 表.
#[test]
fn cp932_experiment_f2_cp932_name_containing_0x5c_second_bytes() {
    let name = [0x83, 0x5C, 0x81, 0x5B, 0x83, 0x58, b'/', 0x95, 0x5C, b'.', b't', b'x', b't'];
    let o = observe("F2 CP932 ソース/表.txt (0x5C inside), no bit 11", ZipBuilder::new().entry(one(&name, 0)).build());
    assert_eq!(o.open, Ok(1));
    assert_eq!(o.entries[0].name_raw, name);
    eprintln!("[F2] contains a backslash after cp437 decoding: {}", o.entries[0].name.contains('\\'));
}

// F3: UTF-8 bytes, bit 11 set.
#[test]
fn cp932_experiment_f3_utf8_name_with_the_flag() {
    let o = observe("F3 UTF-8 テスト.txt, bit 11", ZipBuilder::new().entry(one("テスト.txt".as_bytes(), FLAG_UTF8)).build());
    assert_eq!(o.open, Ok(1));
    assert_eq!(o.entries[0].name, "テスト.txt");
}

// F4: UTF-8 bytes, bit 11 clear.
#[test]
fn cp932_experiment_f4_utf8_name_without_the_flag() {
    let o = observe("F4 UTF-8 テスト.txt, no bit 11", ZipBuilder::new().entry(one("テスト.txt".as_bytes(), 0)).build());
    assert_eq!(o.open, Ok(1));
    assert_eq!(o.entries[0].name_raw, "テスト.txt".as_bytes());
    eprintln!("[F4] name() == \"テスト.txt\": {}", o.entries[0].name == "テスト.txt");
}

// F5: header name CP932, no bit 11, Info-ZIP Unicode Path extra field (0x7075) with the UTF-8 name.
#[test]
fn cp932_experiment_f5_unicode_path_extra_field() {
    let header_name = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'.', b't', b'x', b't'];
    let extra = unicode_path_extra(crc32(&header_name), "テスト.txt");
    let entry = one(&header_name, 0).extra_central(&extra).extra_local(&extra);
    let o = observe("F5 CP932 header name + 0x7075 UTF-8 name", ZipBuilder::new().entry(entry).build());
    assert_eq!(o.open, Ok(1));
    eprintln!("[F5] the crate honours 0x7075: {}", o.entries[0].name == "テスト.txt");
}

// F6: bytes that are not valid CP932 (0x81 0x00), bit 11 clear.
#[test]
fn cp932_experiment_f6_invalid_cp932_bytes() {
    let name = [0x81, 0x00, b'.', b't', b'x', b't'];
    let o = observe("F6 invalid CP932 bytes, no bit 11", ZipBuilder::new().entry(one(&name, 0)).build());
    eprintln!("[F6] open = {:?}", o.open);
}

// F7: local header name differs from the central directory name (R-361-1 / AC-365-1).
#[test]
fn cp932_experiment_f7_local_and_central_names_differ() {
    let entry = Entry::file("central.txt", b"payload").local_name(b"local.txt");
    let o = observe("F7 central=central.txt local=local.txt", ZipBuilder::new().entry(entry).build());
    assert_eq!(o.open, Ok(1));
    eprintln!("[F7] name() (central or local?) = {:?}; by_index read = {:?}", o.entries.first().map(|e| e.name.clone()), o.entries.first().map(|e| e.read.clone()));
    let entry = Entry::file("../evil.txt", b"payload").local_name(b"ok.txt");
    observe("F7b central=../evil.txt local=ok.txt", ZipBuilder::new().entry(entry).build());
}

// ---------------------------------------------------------------------------
// F8-F10 (SPEC-0200-0001 §3.4.13-6)
// ---------------------------------------------------------------------------

// F8: duplicate / case-only-duplicate names -- does the crate reject at open time (MUST-70)?
#[test]
fn cp932_experiment_f8_duplicate_names() {
    let exact = observe("F8a exact duplicate a.txt x2", ZipBuilder::new().file("a.txt", b"1").file("a.txt", b"2").build());
    let cased = observe("F8b case-only duplicate A.txt / a.txt", ZipBuilder::new().file("A.txt", b"1").file("a.txt", b"2").build());
    eprintln!(
        "[F8] exact duplicate -> open {:?} (entries seen: {}); case-only -> open {:?} (entries seen: {})",
        exact.open,
        exact.entries.len(),
        cased.open,
        cased.entries.len()
    );
    eprintln!("[F8] if the exact duplicate is an Err at open time the pipeline must scan the central directory itself (branch 3), else R-364-1 can be done on by_index_raw names");
}

// F9: bit 11 set but the bytes are not UTF-8.
#[test]
fn cp932_experiment_f9_utf8_flag_with_invalid_utf8() {
    let lone = [b'a', 0xED, 0xA0, 0x80, b'.', b't', b'x', b't']; // WTF-8 for U+D800
    let o = observe("F9a bit 11 + WTF-8 lone surrogate", ZipBuilder::new().entry(one(&lone, FLAG_UTF8)).build());
    if let Some(e) = o.entries.first() {
        eprintln!("[F9a] name() = {:?} (lossy replacement char present: {}), name_raw = [{}]", e.name, e.name.contains('\u{FFFD}'), hex(&e.name_raw));
    }
    let junk = [0xFF, 0xFE, b'x'];
    observe("F9b bit 11 + FF FE", ZipBuilder::new().entry(one(&junk, FLAG_UTF8)).build());
}

// F10: Unicode Path extra field whose CRC does not match the header name.
#[test]
fn cp932_experiment_f10_unicode_path_with_wrong_crc() {
    let header_name = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'.', b't', b'x', b't'];
    let extra = unicode_path_extra(crc32(&header_name) ^ 0xFFFF_FFFF, "テスト.txt");
    let entry = one(&header_name, 0).extra_central(&extra).extra_local(&extra);
    let o = observe("F10 0x7075 with a WRONG name CRC", ZipBuilder::new().entry(entry).build());
    if let Some(e) = o.entries.first() {
        eprintln!("[F10] the crate used the (mismatching) extra name: {}", e.name == "テスト.txt");
    }
}

// ---------------------------------------------------------------------------
// Other things the pipeline depends on seeing (§3.4.2 AES / encryption / symlink / methods)
// ---------------------------------------------------------------------------

// AC-360 / AC-360-1: how do ZipCrypto and AES entries look with the codec/encryption features OFF?
#[test]
fn cp932_experiment_encrypted_entries_as_seen_by_by_index_raw() {
    let zipcrypto = observe("E1 ZipCrypto (bit 0)", ZipBuilder::new().entry(Entry::file("enc.txt", b"x").set_flag(FLAG_ENCRYPTED)).build());
    let aes = observe("E2 AES method 99 + bit 0", ZipBuilder::new().entry(Entry::aes("aes.txt", true)).build());
    let aes_bare = observe("E3 AES method 99 without bit 0", ZipBuilder::new().entry(Entry::aes("aes2.txt", false)).build());
    assert_eq!(zipcrypto.open, Ok(1));
    assert!(zipcrypto.entries[0].encrypted, "bit 0 must be visible through by_index_raw().encrypted()");
    for (label, o) in [("E2", &aes), ("E3", &aes_bare)] {
        eprintln!("[{label}] open = {:?}; entries = {:?}", o.open, o.entries.iter().map(|e| (e.method.clone(), e.encrypted)).collect::<Vec<_>>());
    }
}

// AC-367: symlink entries as the crate reports them.
#[test]
fn cp932_experiment_symlink_entries() {
    let o = observe("E4 unix symlink entry", ZipBuilder::new().entry(Entry::symlink("link", "target.txt")).build());
    assert_eq!(o.open, Ok(1));
    assert!(o.entries[0].is_symlink, "S_IFLNK in the unix mode must be reported as a symlink");
    let dos_only = observe("E4b DOS-host entry whose attributes look like a symlink", ZipBuilder::new().entry(Entry::file("x", b"y").dos_attrs(0o120777 << 16)).build());
    eprintln!("[E4b] is_symlink = {:?}", dos_only.entries.first().map(|e| e.is_symlink));
}

// AC-362: unsupported methods with the default codec features OFF.
#[test]
fn cp932_experiment_unsupported_methods() {
    for (label, method) in [("E5 BZIP2 (12)", METHOD_BZIP2), ("E6 LZMA (14)", METHOD_LZMA), ("E7a Deflate64 (9)", 9), ("E7b Zstd (93)", 93)] {
        let o = observe(label, ZipBuilder::new().entry(Entry::file("x.bin", b"data").method(method)).build());
        assert_eq!(o.open, Ok(1), "{label}: by_index_raw must still describe the entry");
    }
}

// AC-362-2: does the crate itself notice multi-disk archives?
#[test]
fn cp932_experiment_multi_disk_eocd() {
    let o = observe("E8 EOCD disks (1,1)", ZipBuilder::new().file("a.txt", b"a").disks(1, 1).build());
    eprintln!("[E8] crate open = {:?} (if Ok, eocd.rs is what rejects multi-volume archives)", o.open);
}

// MUST-70: EOCD count vs central directory.
#[test]
fn cp932_experiment_declared_count_mismatch() {
    let o = observe("E9 EOCD says 3, directory holds 2", ZipBuilder::new().file("a", b"1").file("b", b"2").declare_entry_count(3).declare_entries_on_this_disk(3).build());
    eprintln!("[E9] crate open = {:?}", o.open);
}

// AC-373-1 / §3.4.14 C4: the crate must detect a CRC mismatch by itself (no crc32fast dependency).
#[test]
fn cp932_experiment_crc_mismatch_is_detected_by_the_crate() {
    let stored = observe("E10a Stored, wrong CRC", ZipBuilder::new().entry(Entry::stored("s.txt", b"hello").crc(0xDEAD_BEEF)).build());
    let deflated = observe("E10b Deflate, wrong CRC", ZipBuilder::new().entry(Entry::file("d.txt", b"hello deflate").crc(0x1234_5678)).build());
    assert!(stored.entries[0].read.is_err(), "Stored + bad CRC must fail while reading: {:?}", stored.entries[0].read);
    assert!(deflated.entries[0].read.is_err(), "Deflate + bad CRC must fail while reading: {:?}", deflated.entries[0].read);
    // and the good ones read fine, so the failure above is the CRC and not the fixture
    let good = observe("E10c good CRCs", ZipBuilder::new().entry(Entry::stored("s.txt", b"hello")).file("d.txt", b"hello deflate").build());
    assert_eq!(good.entries[0].read, Ok(5));
    assert_eq!(good.entries[1].read, Ok(13));
}

// L-4 background: what does the reader do when the header says fewer / more bytes than the stream holds?
#[test]
fn cp932_experiment_declared_size_versus_real_size() {
    let real = b"0123456789";
    let comp = deflate_stored(real).len() as u64;
    let smaller = observe("E11a Deflate stream = 10 bytes, header says 3", ZipBuilder::new().entry(Entry::file("s.txt", real).declare_sizes(3, comp)).build());
    let larger = observe("E11b Deflate stream = 3 bytes, header says 10", ZipBuilder::new().entry(Entry::file("l.txt", b"abc").declare_sizes(10, deflate_stored(b"abc").len() as u64)).build());
    eprintln!("[E11a] read = {:?}", smaller.entries.first().map(|e| e.read.clone()));
    eprintln!("[E11b] read = {:?}", larger.entries.first().map(|e| e.read.clone()));
    let zero = observe("E11c Deflate, compressed 0 / uncompressed 0", ZipBuilder::new().entry(Entry::declared("e.txt", METHOD_DEFLATE, 0, 0)).build());
    eprintln!("[E11c] read = {:?}", zero.entries.first().map(|e| e.read.clone()));
}

// R-346-2: how the crate reports DOS timestamps (valid / zero).
#[test]
fn cp932_experiment_dos_timestamps() {
    let o = observe(
        "E12 mtime 2020-01-02 03:04:06 / date 0",
        ZipBuilder::new()
            .entry(Entry::file("dated.txt", b"x").dos_datetime(dos_date(2020, 1, 2), dos_time(3, 4, 6)))
            .entry(Entry::file("undated.txt", b"y").dos_datetime(0, 0))
            .build(),
    );
    assert_eq!(o.open, Ok(2));
}

// ZIP64: 70,000 entries opens (R-346-1); entry with zip64 extra sizes.
#[test]
fn cp932_experiment_zip64_shapes() {
    let mut b = ZipBuilder::new();
    for i in 0..70_000 {
        b = b.entry(Entry::stored(&format!("f{i}"), b""));
    }
    let big = zip::ZipArchive::new(Cursor::new(b.build())).map(|a| a.len());
    eprintln!("[E13] 70,000 empty entries -> {big:?}");
    assert_eq!(big.ok(), Some(70_000));
}

/* OBSERVATIONS (zip 7.2.0, features deflate+time; recorded 2026-09-27 by unit-test-writer)
 F1/F2 CP932 bytes, no bit 11: name() is CP437-decoded mojibake (F2 contains a real backslash from 0x5C); name_raw() returns the exact bytes -> BRANCH 1 feasible (name_raw + encoding_rs).
 F3 bit 11 + UTF-8: name() correct. F4 UTF-8 bytes, no bit 11: name() is CP437 mojibake, name_raw() is the UTF-8 bytes -> own "valid UTF-8 => UTF-8" decision needed.
 F5 Unicode Path (0x7075) with correct CRC: crate REPLACES name and name_raw with the UTF-8 extra name (so the header CP932 bytes are not visible any more).
 F6 invalid CP932 bytes: open OK, name() is CP437 text; name_raw exact.
 F7 local!=central: name() is the CENTRAL name (central.txt / ../evil.txt); local name ignored by the crate, by_index reads fine.
 F8 EXACT DUPLICATE names: ZipArchive::new opens OK with len 1 (crate silently DEDUPLICATES; the second entry is invisible) -> R-364-1 needs an own central-directory name scan (design branch 3) before/instead of relying on the crate. Case-only duplicates (A.txt/a.txt): both visible (len 2).
 F9 bit 11 + invalid UTF-8: open OK, name() lossy (U+FFFD), name_raw exact -> own strict UTF-8 check on name_raw gives invalid_path.
 F10 Unicode Path with WRONG CRC: ZipArchive::new fails ("CRC32 checksum failed on Unicode extra field") -> would be classified zip_corrupt.
 E1 bit 0: by_index_raw().encrypted() == true. E2 AES+bit0: encrypted() true. E3 AES method 99 WITHOUT bit 0: encrypted() false and compression() reports Deflated (the 0x9901 extra is honoured), by_index says "password incorrect" -> method 99 is NOT visible via the crate: detect AES from the central directory / extra field 0x9901 yourself.
 E4 unix symlink: is_symlink() true. E4b DOS-host entry whose external attrs high word looks like S_IFLNK: is_symlink() also true (crate ignores host).
 E5-E7 BZIP2/LZMA/Deflate64/Zstd: open OK; by_index -> UnsupportedArchive; compression() gives the method (check via by_index_raw).
 E8 multi-disk EOCD (1,1): crate opens it fine -> eocd.rs must reject. E9 EOCD count 3 vs 2 real: crate Err (InvalidArchive) -> zip_corrupt.
 E10 CRC mismatch (Stored and Deflate): read_to_end returns Err(InvalidData "Invalid checksum") -> C4 confirmed, crc32fast not needed.
 E11a Deflate stream 10 bytes but header says 3: read Ok(10) (crate does NOT clamp) -> L-4 (declared+1 take) must be done by us. E11b stream 3 header 10: read Ok(3) (no error) -> "fewer than declared" must be detected by us. E11c compressed 0 / uncompressed 0 Deflate: read FAILS (UnexpectedEof) -> AC-374-1 needs a special case (declared size 0 => create empty file without reading).
 E13 70,000 entries (ZIP64 EOCD) opens. */
