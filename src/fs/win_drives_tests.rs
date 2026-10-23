//! TDD (unit-test-writer, 2026-09-30): `fs::win_drives::bitmask_to_drive_roots`
//! (SPEC-0300-0016 R-780/R-770-1; SPEC-0200-0001 §3.7.1, §3.7.10.9
//! SHOULD-139(e)). Pure bit-to-string-array conversion, no OS call — these
//! tests run on every platform (unlike `list_logical_drives`, which requires
//! `#[cfg(windows)]` and is exercised only by AC-754's real-machine check).
//!
//! Expected to fail (RED, `unimplemented!()` panics) until code-generator
//! fills in the function body.

use super::win_drives::bitmask_to_drive_roots;

// R-780: no drives set -> empty result.
#[test]
fn empty_mask_yields_no_drives() {
    assert_eq!(bitmask_to_drive_roots(0), Vec::<String>::new());
}

// R-780: bit 0 is drive A (the lowest letter), formatted as "A:\".
#[test]
fn bit_0_is_drive_a() {
    assert_eq!(bitmask_to_drive_roots(1), vec!["A:\\".to_string()]);
}

// R-780: bit 25 is drive Z (the highest letter).
#[test]
fn bit_25_is_drive_z() {
    let mask = 1u32 << 25;
    assert_eq!(bitmask_to_drive_roots(mask), vec!["Z:\\".to_string()]);
}

// R-770-1: multiple set bits are returned in ascending drive-letter order,
// regardless of which bit happens to be numerically larger elsewhere in the
// mask (C = bit 2, D = bit 3, Z = bit 25 here, deliberately out of the order
// they are OR'd together below).
#[test]
fn multiple_drives_are_returned_in_ascending_order() {
    let mask = (1u32 << 25) | (1u32 << 3) | (1u32 << 2); // Z, D, C
    assert_eq!(
        bitmask_to_drive_roots(mask),
        vec!["C:\\".to_string(), "D:\\".to_string(), "Z:\\".to_string()]
    );
}

// AC-730/R-770: the middle-of-the-alphabet worked example from the spec's
// own AC (C: and D:).
#[test]
fn ac730_c_and_d_present_only() {
    let mask = (1u32 << 2) | (1u32 << 3); // C, D
    assert_eq!(
        bitmask_to_drive_roots(mask),
        vec!["C:\\".to_string(), "D:\\".to_string()]
    );
}

// SHOULD-139(e): bits 26-31 (beyond 'Z') do not exist for GetLogicalDrives,
// but a future Windows quirk or test double could still set them -- they
// must never surface as a drive.
#[test]
fn bits_26_to_31_are_ignored() {
    let mask = (1u32 << 2) | (1u32 << 26) | (1u32 << 31); // C, plus 2 out-of-range bits
    assert_eq!(bitmask_to_drive_roots(mask), vec!["C:\\".to_string()]);
}

// R-780: a fully-set 32-bit mask still yields exactly the 26 letters A..Z,
// in order, with none of the high bits leaking through.
#[test]
fn a_fully_set_mask_yields_exactly_the_26_letters_in_order() {
    let expected: Vec<String> = ('A'..='Z').map(|c| format!("{c}:\\")).collect();
    assert_eq!(bitmask_to_drive_roots(u32::MAX), expected);
}
