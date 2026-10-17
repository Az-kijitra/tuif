//! TDD (unit-test-writer, 2026-09-27): `archive::entry_name::resolve_entry_name`.
//!
//! Specs: SPEC-0300-0014 R-347 (policy: UTF-8 flag -> UTF-8; no flag -> UTF-8 when
//! valid else CP932), R-363 #11 (`invalid_path`), AC-329, AC-364-3;
//! SPEC-0200-0001 §3.4.9-c.
//!
//! Only the UTF-8 half of the policy is pinned now. The CP932 half needs
//! `encoding_rs` (approved only if the F1-F10 experiment selects branch 1,
//! `cp932_experiment_tests.rs`); those tests were `#[ignore]`d until the experiment (now enabled) with the exact
//! expected values so they can be enabled after the experiment.

use super::entry_name::resolve_entry_name;
use super::error::ArchiveErrorCode;

// R-347: bit 11 set -> the bytes are UTF-8.
#[test]
fn r347_utf8_flag_set_decodes_as_utf8() {
    assert_eq!(resolve_entry_name("テスト.txt".as_bytes(), true).unwrap(), "テスト.txt");
    assert_eq!(resolve_entry_name(b"plain.txt", true).unwrap(), "plain.txt");
    assert_eq!(resolve_entry_name("日本語/😀.txt".as_bytes(), true).unwrap(), "日本語/😀.txt");
}

// R-363 #11 / AC-364-3: bit 11 set but not valid UTF-8 (e.g. a WTF-8/CESU-8
// encoded lone surrogate U+D800, or arbitrary bytes) -> `invalid_path`.
#[test]
fn ac364_3_utf8_flag_set_with_invalid_utf8_is_invalid_path() {
    let lone_surrogate = [b'a', 0xED, 0xA0, 0x80, b'.', b't', b'x', b't'];
    let garbage = [0xFF, 0xFE, b'x'];
    for bytes in [&lone_surrogate[..], &garbage[..]] {
        let err = resolve_entry_name(bytes, true).expect_err("invalid UTF-8 with bit 11 must be rejected");
        assert_eq!(err.code, ArchiveErrorCode::InvalidPath);
    }
}

// R-347: bit 11 clear but the bytes ARE valid UTF-8 -> UTF-8 (F4).
#[test]
fn r347_no_flag_but_valid_utf8_is_decoded_as_utf8() {
    assert_eq!(resolve_entry_name("テスト.txt".as_bytes(), false).unwrap(), "テスト.txt");
    assert_eq!(resolve_entry_name(b"ascii.txt", false).unwrap(), "ascii.txt");
}

// The resolver only decodes: `\`, `..`, `:` etc. are validated afterwards by
// `path_rules` (R-347: "解釈後の名前に対して §6 の検証を適用する").
#[test]
fn r347_the_resolver_does_not_validate_or_alter_path_syntax() {
    assert_eq!(resolve_entry_name(b"../x", true).unwrap(), "../x");
    assert_eq!(resolve_entry_name(br"a\b:c", false).unwrap(), r"a\b:c");
}

// ---- CP932 (pending the F1-F10 experiment; requires `encoding_rs`) ----------

// AC-329 / F1: CP932 bytes, no bit 11.
#[test]
fn ac329_cp932_bytes_without_the_flag_are_decoded_as_cp932() {
    let bytes = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'.', b't', b'x', b't']; // テスト.txt
    assert_eq!(resolve_entry_name(&bytes, false).unwrap(), "テスト.txt");
}

// F2: 0x5C as the second byte of 'ソ' / '表' must NOT become a path separator.
#[test]
fn ac329_cp932_second_byte_0x5c_is_not_a_separator() {
    // ソース/表.txt
    let bytes = [0x83, 0x5C, 0x81, 0x5B, 0x83, 0x58, b'/', 0x95, 0x5C, b'.', b't', b'x', b't'];
    assert_eq!(resolve_entry_name(&bytes, false).unwrap(), "ソース/表.txt");
}
