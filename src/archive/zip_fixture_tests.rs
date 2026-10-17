//! Self-tests of the hand-assembled ZIP builder (`zip_fixture.rs`): they show
//! that the fixtures are valid ZIPs for the `zip` crate, so a later RED/GREEN
//! result of the real tests is about the implementation, not about a broken
//! fixture. These do not depend on any production stub and are expected GREEN.
//!
//! Specs: SPEC-0200-0001 §3.4.10 (`zip_fixture.rs`), SPEC-0300-0014 AC-328-1
//! (ZIP64), AC-360 / AC-360-1 / AC-362 / AC-367 fixtures.

use super::zip_fixture::*;
use std::io::{Cursor, Read};

fn open(bytes: Vec<u8>) -> zip::ZipArchive<Cursor<Vec<u8>>> {
    zip::ZipArchive::new(Cursor::new(bytes)).expect("fixture must be a valid ZIP for the zip crate")
}

#[test]
fn crc32_matches_the_standard_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn deflate_stored_blocks_inflate_correctly_for_small_and_multi_block_data() {
    for len in [0usize, 1, 100, 65_535, 65_536, 200_000] {
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let zip = ZipBuilder::new().file("d.bin", &data).build();
        let mut archive = open(zip);
        let mut file = archive.by_index(0).expect("entry");
        let mut back = Vec::new();
        file.read_to_end(&mut back).expect("inflate + crc");
        assert_eq!(back, data, "len {len}");
    }
}

#[test]
fn builder_output_opens_with_names_kinds_and_data() {
    let bytes = ZipBuilder::new()
        .dir("docs")
        .file("docs/a.txt", b"hello")
        .entry(Entry::stored("docs/b.txt", b"stored!"))
        .file("日本語.txt", "こんにちは".as_bytes())
        .build();
    let mut archive = open(bytes);
    assert_eq!(archive.len(), 4);
    let names: Vec<String> = (0..4).map(|i| archive.by_index_raw(i).unwrap().name().to_string()).collect();
    assert_eq!(names, ["docs/", "docs/a.txt", "docs/b.txt", "日本語.txt"]);
    assert!(archive.by_index_raw(0).unwrap().is_dir());
    let mut s = String::new();
    archive.by_index(2).unwrap().read_to_string(&mut s).unwrap();
    assert_eq!(s, "stored!");
    let mut s = String::new();
    archive.by_index(3).unwrap().read_to_string(&mut s).unwrap();
    assert_eq!(s, "こんにちは");
}

#[test]
fn independent_parser_sees_flags_methods_and_names() {
    let bytes = ZipBuilder::new().file("plain.txt", b"x").file("日本.txt", b"y").entry(Entry::stored("s.txt", b"z")).build();
    let cd = parse_central_directory(&bytes);
    assert_eq!(cd.len(), 3);
    assert_eq!(cd[0].name_str(), "plain.txt");
    assert_eq!(cd[0].flags & FLAG_UTF8, 0, "ASCII names carry no bit 11");
    assert_eq!(cd[1].flags & FLAG_UTF8, FLAG_UTF8, "non-ASCII UTF-8 names carry bit 11");
    assert_eq!(cd[0].method, METHOD_DEFLATE);
    assert_eq!(cd[2].method, METHOD_STORED);
}

#[test]
fn forged_sizes_and_crc_are_written_as_given() {
    let bytes = ZipBuilder::new().entry(Entry::stored("a.txt", b"hello").declare_sizes(3, 3).crc(0xDEAD_BEEF)).build();
    let cd = parse_central_directory(&bytes);
    assert_eq!((cd[0].uncompressed, cd[0].compressed, cd[0].crc32), (3, 3, 0xDEAD_BEEF));
}

#[test]
fn encryption_flag_method_99_and_symlink_attributes_are_visible_to_the_crate() {
    let bytes = ZipBuilder::new()
        .entry(Entry::file("enc.txt", b"x").set_flag(FLAG_ENCRYPTED))
        .entry(Entry::aes("aes.txt", true))
        .entry(Entry::aes("aes-nobit0.txt", false))
        .entry(Entry::symlink("link", "target.txt"))
        .entry(Entry::file("bz.txt", b"x").method(METHOD_BZIP2))
        .build();
    let mut archive = open(bytes);
    assert!(archive.by_index_raw(0).unwrap().encrypted());
    let aes = archive.by_index_raw(1).unwrap();
    assert!(aes.encrypted());
    drop(aes);
    let link = archive.by_index_raw(3).unwrap();
    assert!(link.is_symlink(), "unix mode S_IFLNK must be visible as a symlink");
    drop(link);
    let cd = parse_central_directory(&ZipBuilder::new().entry(Entry::aes("a", false)).build());
    assert_eq!(cd[0].method, METHOD_AES);
    assert_eq!(cd[0].flags & FLAG_ENCRYPTED, 0, "the bit-0-less AES variant really has bit 0 clear");
}

#[test]
fn local_name_forgery_keeps_the_central_name() {
    let bytes = ZipBuilder::new().entry(Entry::file("ok.txt", b"x").local_name(b"../evil.txt")).build();
    let cd = parse_central_directory(&bytes);
    assert_eq!(cd[0].name_str(), "ok.txt");
    assert!(bytes.windows(11).any(|w| w == b"../evil.txt"), "the local header carries the forged name");
}

#[test]
fn forced_zip64_entry_and_eocd_open_and_read_back() {
    let bytes = ZipBuilder::new().entry(Entry::file("z64.txt", b"zip64 data").zip64()).force_zip64_eocd().build();
    let mut archive = open(bytes);
    let mut s = String::new();
    archive.by_index(0).unwrap().read_to_string(&mut s).unwrap();
    assert_eq!(s, "zip64 data");
}

// AC-328-1 fixture shape: > 65,535 empty entries (headers only) need the ZIP64 EOCD.
#[test]
fn seventy_thousand_entries_produce_a_zip64_archive_the_crate_can_open() {
    let mut builder = ZipBuilder::new();
    for i in 0..70_000 {
        builder = builder.entry(Entry::stored(&format!("f{i}.txt"), b""));
    }
    let archive = open(builder.build());
    assert_eq!(archive.len(), 70_000);
}

// L-2 / L-5 fixtures: huge DECLARED sizes with physically consistent, tiny data.
#[test]
fn declared_size_fixture_reports_the_declared_sizes() {
    let eight_gib = 8u64 << 30;
    let compressed = eight_gib / 1032 + 1;
    let bytes = ZipBuilder::new().entry(Entry::declared("big.bin", METHOD_DEFLATE, eight_gib, compressed)).build();
    let mut archive = open(bytes);
    let f = archive.by_index_raw(0).unwrap();
    assert_eq!(f.size(), eight_gib);
    assert_eq!(f.compressed_size(), compressed);
}

#[test]
fn unicode_path_extra_field_has_the_documented_layout() {
    let extra = unicode_path_extra(0x1234_5678, "abc");
    assert_eq!(&extra[..2], &[0x75, 0x70]);
    assert_eq!(u16::from_le_bytes([extra[2], extra[3]]) as usize, 1 + 4 + 3);
    assert_eq!(extra[4], 1);
    assert_eq!(&extra[5..9], &0x1234_5678u32.to_le_bytes());
    assert_eq!(&extra[9..], b"abc");
}

#[test]
fn dos_date_and_time_encode_as_documented() {
    assert_eq!(dos_date(2020, 1, 2), ((2020 - 1980) << 9) | (1 << 5) | 2);
    assert_eq!(dos_time(3, 4, 6), (3 << 11) | (4 << 5) | 3);
}
