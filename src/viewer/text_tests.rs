//! TDD (unit-test-writer, 2026-09-27): encoding auto-detection (R-450),
//! replacement-character rules for undecodable/incomplete tails (R-450-1,
//! R-450-4, R-453), the 2 MiB / 120,000-line caps (R-445, R-445-4) and the
//! R-445-2 truncation message format.
//!
//! Specs: SPEC-0300-0015 R-445, R-445-1~4, R-450~R-454; AC-455, AC-455-1,
//! AC-455-2, AC-460~AC-467.
//!
//! `decode(bytes, truncated_by_size)` is tested directly with in-memory byte
//! buffers for everything except the ">2 MiB decides truncated_by_size"
//! boundary itself, which is `read_text`'s job and is tested with `tempfile`
//! (that boundary needs a real file so `read_text` can compare against the
//! actual on-disk size).

use super::text::*;
use std::io::Write;

fn utf16le_bom_plus(units: &[u16]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

fn utf16be_bom_plus(units: &[u16]) -> Vec<u8> {
    let mut out = vec![0xFE, 0xFF];
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------
// R-450: encoding auto-detection
// ---------------------------------------------------------------------

#[test]
fn r450_step4_ascii_only_bytes_with_no_bom_are_utf8() {
    let payload = decode(b"hello, world\n", false);
    assert_eq!(payload.encoding, EncodingLabel::Utf8);
    assert_eq!(payload.content, "hello, world\n");
}

#[test]
fn r450_step4_valid_multibyte_utf8_with_no_bom_is_utf8() {
    // AC-460: Japanese text, no BOM.
    let payload = decode("こんにちは".as_bytes(), false);
    assert_eq!(payload.encoding, EncodingLabel::Utf8);
    assert_eq!(payload.content, "こんにちは");
}

#[test]
fn r450_step1_a_utf8_bom_is_stripped_and_labeled_bom() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice("hi".as_bytes());
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::Utf8Bom);
    assert_eq!(payload.content, "hi", "the BOM itself must not appear in content");
}

#[test]
fn r450_step2_a_utf16le_bom_is_decoded_and_stripped() {
    // "AB" as UTF-16LE code units.
    let bytes = utf16le_bom_plus(&[0x0041, 0x0042]);
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::Utf16Le);
    assert_eq!(payload.content, "AB");
}

#[test]
fn r450_step3_a_utf16be_bom_is_decoded_and_stripped() {
    let bytes = utf16be_bom_plus(&[0x0041, 0x0042]);
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::Utf16Be);
    assert_eq!(payload.content, "AB");
}

#[test]
fn r450_step5_non_utf8_bytes_fall_back_to_cp932() {
    // AC-460: Shift_JIS bytes for the CP932-only extended characters "①" and
    // "髙", not valid UTF-8 (SPEC-0300-0014 R-347's approach: valid UTF-8 wins,
    // otherwise CP932, R-450-3's WHATWG Shift_JIS decoder).
    let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode("①髙");
    assert!(!had_errors, "test fixture: both characters must be representable in CP932");
    assert!(std::str::from_utf8(&bytes).is_err(), "test fixture: must not accidentally be valid UTF-8");
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::ShiftJis);
    assert_eq!(payload.content, "①髙");
}

#[test]
fn r450_1_an_undecodable_cp932_byte_becomes_u_fffd_and_decoding_continues() {
    // AC-461: bytes that are invalid both as UTF-8 and as CP932 surface as
    // U+FFFD without aborting the rest of the text.
    //
    // Test fix (code-generator, 2026-09-27): the original fixture used 0x80,
    // but the WHATWG Shift_JIS decoder that R-450-3 mandates (the same
    // `encoding_rs` decoder already used for ZIP entry names, SPEC-0300-0014
    // R-347) defines byte 0x80 (with no pending lead byte) as a compatibility
    // mapping to U+0080, NOT an error -- verified against `encoding_rs`'s
    // actual (spec-compliant) output, which is `\u{80}`, not `\u{FFFD}`. The
    // premise "0x80 ... undefined" was factually wrong for the decoder R-450-3
    // requires. 0xFD IS outside every defined Shift_JIS byte range (ASCII,
    // 0x80, halfwidth katakana 0xA1-0xDF, lead bytes 0x81-0x9F/0xE0-0xFC), so
    // it genuinely triggers the WHATWG "undefined byte -> error -> U+FFFD"
    // path this test intends to pin; the rest of the test (content
    // continuing before/after, encoding=ShiftJis) is unchanged.
    let mut bytes = b"before-".to_vec();
    bytes.push(0xFD);
    bytes.extend_from_slice(b"-after");
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::ShiftJis);
    assert!(payload.content.contains('\u{FFFD}'), "content={:?}", payload.content);
    assert!(payload.content.starts_with("before-"));
    assert!(payload.content.ends_with("-after"));
}

// ---------------------------------------------------------------------
// R-453: truncated-tail handling (only when truncated_by_size = true)
// ---------------------------------------------------------------------

#[test]
fn r453_a_truncated_utf8_drops_an_incomplete_trailing_multibyte_sequence() {
    // AC-462/AC-463: "あ" is E3 81 82; keep only the first two bytes (E3 81),
    // simulating a 2 MiB cut landing mid-character.
    let mut bytes = b"ok-".to_vec();
    bytes.extend_from_slice(&[0xE3, 0x81]); // incomplete tail of "あ"
    let payload = decode(&bytes, true);
    assert_eq!(payload.encoding, EncodingLabel::Utf8, "step4 must not be defeated by the incomplete tail");
    assert_eq!(payload.content, "ok-");
    assert!(!payload.content.contains('\u{FFFD}'), "R-453(d): no replacement character for a dropped tail");
}

#[test]
fn r453_a_not_truncated_the_same_incomplete_tail_is_invalid_utf8_and_falls_to_cp932() {
    // AC-463: without truncation, an incomplete trailing sequence disqualifies
    // step4 entirely (R-453(a) last sentence) -- the WHOLE buffer is then
    // treated as CP932 (R-450 順5), not "UTF-8 minus the bad tail".
    let mut bytes = b"ok-".to_vec();
    bytes.extend_from_slice(&[0xE3, 0x81]);
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::ShiftJis);
}

#[test]
fn r453_b_truncated_utf16le_with_an_odd_byte_count_drops_the_last_byte_silently() {
    // AC-464.
    let mut bytes = utf16le_bom_plus(&[0x0041, 0x0042]); // "AB"
    bytes.push(0x99); // stray odd trailing byte
    let payload = decode(&bytes, true);
    assert_eq!(payload.encoding, EncodingLabel::Utf16Le);
    assert_eq!(payload.content, "AB");
    assert!(!payload.content.contains('\u{FFFD}'));
}

#[test]
fn r453_b_truncated_utf16le_with_an_unpaired_trailing_high_surrogate_drops_it() {
    // AC-464: 0xD800 is a lone (unpaired) high surrogate with nothing after it.
    let bytes = utf16le_bom_plus(&[0x0041, 0xD800]);
    let payload = decode(&bytes, true);
    assert_eq!(payload.encoding, EncodingLabel::Utf16Le);
    assert_eq!(payload.content, "A");
    assert!(!payload.content.contains('\u{FFFD}'));
}

#[test]
fn r450_4_not_truncated_an_odd_byte_count_utf16le_keeps_the_byte_as_u_fffd() {
    // AC-464-1 (L-6): the SAME malformed tail, but the file was read in full
    // (not truncated) -- R-450-4 says keep it, replaced by U+FFFD.
    let mut bytes = utf16le_bom_plus(&[0x0041]);
    bytes.push(0x99);
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::Utf16Le);
    assert!(payload.content.starts_with('A'));
    assert!(payload.content.contains('\u{FFFD}'), "content={:?}", payload.content);
}

#[test]
fn r453_c_truncated_cp932_drops_a_trailing_lead_byte_with_no_pair() {
    // AC-465: 0x93 is a valid CP932 lead byte; cut before its trailing byte.
    let (mut bytes, _, _) = { let (b, e, h) = encoding_rs::SHIFT_JIS.encode("ok-"); (b.into_owned(), e, h) };
    bytes.push(0x93); // lead byte only, no trailing byte
    let payload = decode(&bytes, true);
    assert_eq!(payload.encoding, EncodingLabel::ShiftJis);
    assert_eq!(payload.content, "ok-");
    assert!(!payload.content.contains('\u{FFFD}'));
}

#[test]
fn r450_4_not_truncated_a_cp932_trailing_lead_byte_becomes_u_fffd() {
    // AC-464-1 (L-6): same lead-byte tail, file read in full -> U+FFFD, kept.
    let (mut bytes, _, _) = { let (b, e, h) = encoding_rs::SHIFT_JIS.encode("ok-"); (b.into_owned(), e, h) };
    bytes.push(0x93);
    let payload = decode(&bytes, false);
    assert_eq!(payload.encoding, EncodingLabel::ShiftJis);
    assert!(payload.content.starts_with("ok-"));
    assert!(payload.content.contains('\u{FFFD}'));
}

// ---------------------------------------------------------------------
// R-452: newline normalization (all three styles count as one newline each)
// ---------------------------------------------------------------------

#[test]
fn r452_crlf_lf_and_cr_are_each_one_line_break() {
    // AC-466.
    let payload = decode(b"a\r\nb\nc\rd", false);
    assert_eq!(payload.line_count, 4);
}

// ---------------------------------------------------------------------
// R-445-4: the 120,000-line display cap
// ---------------------------------------------------------------------

#[test]
fn r445_4_exactly_120_000_lines_is_not_truncated() {
    // AC-455-2 (second half): 120,000 lines exactly -> no line truncation.
    let text = "x\n".repeat(MAX_TEXT_LINES);
    let payload = decode(text.as_bytes(), false);
    assert!(!payload.truncated_by_lines);
    assert_eq!(payload.line_count, MAX_TEXT_LINES);
}

#[test]
fn r445_4_over_120_000_lines_is_truncated_to_the_first_120_000() {
    // AC-455-2 (first half).
    let text = "x\n".repeat(MAX_TEXT_LINES + 1);
    let payload = decode(text.as_bytes(), false);
    assert!(payload.truncated_by_lines);
    assert_eq!(payload.line_count, MAX_TEXT_LINES);
    assert_eq!(payload.content.lines().count(), MAX_TEXT_LINES);
}

// ---------------------------------------------------------------------
// R-463: an empty file is one empty line, not an error.
// ---------------------------------------------------------------------

#[test]
fn r463_an_empty_buffer_decodes_as_utf8_with_zero_or_one_line_and_empty_content() {
    let payload = decode(b"", false);
    assert_eq!(payload.encoding, EncodingLabel::Utf8);
    assert_eq!(payload.content, "");
}

// ---------------------------------------------------------------------
// R-445-2: the truncation message wording/rounding
// ---------------------------------------------------------------------

#[test]
fn r445_2_not_truncated_has_no_message() {
    assert_eq!(truncation_message(2_097_152, false), None);
}

#[test]
fn r445_2_5_mib_exactly_formats_without_a_spurious_decimal_bump() {
    // AC-455: 5,242,880 bytes = 5.0 MiB exactly.
    let msg = truncation_message(5_242_880, true).expect("message");
    assert!(msg.contains("5.0 MiB"), "{msg}");
    assert!(msg.contains("5,242,880"), "{msg}");
    assert!(msg.contains("2 MiB"), "{msg}");
}

#[test]
fn r445_2_one_byte_over_2_mib_rounds_up_to_2_1_mib_not_2_0() {
    // AC-455 (L-1 fix): 2,097,153 bytes must show "2.1 MiB" (ceil), never the
    // "2.0 MiB" that a naive round-half-up would produce.
    let msg = truncation_message(2_097_153, true).expect("message");
    assert!(msg.contains("2.1 MiB"), "{msg}");
    assert!(msg.contains("2,097,153"), "{msg}");
}

// ---------------------------------------------------------------------
// read_text: the file-size boundary that decides `truncated_by_size` itself
// (needs a real file; decode() above assumes that decision was already made).
// ---------------------------------------------------------------------

#[test]
fn read_text_exactly_at_the_2_mib_limit_is_not_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    let content = vec![b'a'; MAX_TEXT_BYTES as usize];
    f.write_all(&content).unwrap();
    drop(f);

    let payload = read_text(&path).expect("read_text");
    assert!(!payload.truncated_by_size);
    assert_eq!(payload.original_size_bytes, MAX_TEXT_BYTES);
    assert_eq!(payload.content.len() as u64, MAX_TEXT_BYTES);
}

#[test]
fn read_text_one_byte_over_the_limit_is_truncated_to_the_first_2_mib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    let content = vec![b'a'; (MAX_TEXT_BYTES + 1) as usize];
    f.write_all(&content).unwrap();
    drop(f);

    let payload = read_text(&path).expect("read_text");
    assert!(payload.truncated_by_size);
    assert_eq!(payload.original_size_bytes, MAX_TEXT_BYTES + 1);
    assert_eq!(payload.content.len() as u64, MAX_TEXT_BYTES);
}
