//! TDD (unit-test-writer, 2026-10-07): the one backend "is this an absolute path" judgement.
//! Specs: SPEC-0300-0016 第4版 R-722-3 / AC-757 / AC-758; SPEC-0300-0011 第9版 R-89-6 / R-15-5 /
//! AC-87 / T-01 / T-02 / T-12; SPEC-0200-0001 §3.12.2 / §3.12.9.
//!
//! This file takes over the `is_adoptable` tables of `fs_ops/clipboard/paths_tests.rs` (AC-06-7,
//! R-15-5 / CB-02) -- the same values against `is_absolute_path` -- and adds the AC-757 table.
//!
//! AC-758: the TypeScript `app/src/lib/features/path-bar/normalize.test.ts` holds the SAME
//! representative values (`AC758_TABLE` there); keep the two tables in step. Length and
//! strict-GUID values are NOT in that comparison (R-722-3 M-2 (a) / MINOR-9); they are only here.
//!

use super::abs_path::{first_relative, is_absolute_path, is_absolute_path_buf, INVALID_PATH_CODE, MAX_PATH_UTF16};

const GUID: &str = "12345678-1234-1234-1234-123456789abc";
const SPEC_GUID: &str = "01234567-89ab-cdef-0123-456789abcdef";

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

// ---- AC-757 / AC-758: the shared table (same values in normalize.test.ts) -------------------

/// Absolute paths (AC-757 "絶対パスとするもの").
fn ac757_accepted() -> Vec<String> {
    vec![
        r"C:\".into(),
        r"C:\Users".into(),
        r"c:\a\".into(),
        r"C:\a/b".into(), // a `/` AFTER the root part is not looked at (T-02, 再レビュー M-1 (a))
        r"\\server\share".into(),
        r"\\server\share\".into(),
        r"\\server\share\a".into(),
        r"\\?\C:\a".into(),
        r"\\?\UNC\server\share\a".into(),
        format!(r"\\?\Volume{{{SPEC_GUID}}}\"),
        r"\\.\C:\a".into(),
        format!(r"\\.\Volume{{{SPEC_GUID}}}\a"),
    ]
}

/// Not absolute (AC-757 "絶対パスでないもの"), except the 32,768-unit one which is Rust-only.
fn ac757_rejected() -> Vec<String> {
    vec![
        String::new(),
        "C:".into(),
        "C:src".into(),
        "src".into(),
        r".\src".into(),
        "..".into(),
        r"\foo".into(),
        "/foo".into(),
        r"\\server".into(),
        r"\\server\".into(),
        r"\\.\PhysicalDrive0".into(),
        r"\\.\pipe\x".into(),
        r"\\.\".into(),
        r"\\?\".into(),
        format!(r"C:\{}", "a".repeat(32_768 - 3)), // 32,768 UTF-16 units (Rust only; no limit in the path bar)
        "C:/foo".into(),                           // T-02
    ]
}

/// Known limitation T-12: device paths behind `\\?\` ARE accepted (fixed so that nobody tightens it by accident).
fn t12_accepted() -> Vec<String> {
    vec![r"\\?\PhysicalDrive0".into(), r"\\?\pipe\x".into(), r"\\?\C:".into()]
}

#[test]
fn ac757_every_representative_absolute_path_is_accepted() {
    for p in ac757_accepted() {
        assert!(is_absolute_path(&p), "must be absolute: {p:?}");
    }
}

#[test]
fn ac757_every_representative_non_absolute_path_is_rejected() {
    for p in ac757_rejected() {
        let shown: String = p.chars().take(40).collect();
        assert!(!is_absolute_path(&p), "must NOT be absolute: {shown:?} (len {})", utf16_len(&p));
    }
}

#[test]
fn ac757_t12_device_paths_behind_the_extended_prefix_are_a_known_limitation_and_stay_accepted() {
    for p in t12_accepted() {
        assert!(is_absolute_path(&p), "T-12: {p:?} stays accepted");
    }
}

#[test]
fn ac757_the_table_is_not_empty_and_has_no_value_on_both_sides() {
    let accepted = ac757_accepted();
    let rejected = ac757_rejected();
    assert_eq!(accepted.len(), 12);
    assert_eq!(rejected.len(), 16);
    for a in &accepted {
        assert!(!rejected.contains(a), "{a:?} is on both sides");
    }
}

// ---- constants ---------------------------------------------------------------------------------

#[test]
fn constants_are_the_specified_values() {
    assert_eq!(MAX_PATH_UTF16, 32_767);
    assert_eq!(INVALID_PATH_CODE, "invalid_path");
}

// ---- the former `is_adoptable` tables (AC-06-7 / R-15-5 / CB-02), now against `is_absolute_path` ----

#[test]
fn accepts_drive_unc_extended_and_device_namespace_forms() {
    for p in [
        r"C:\x".to_string(),
        r"z:\dir\file.txt".to_string(),
        r"\\server\share\f".to_string(),
        r"\\?\C:\x".to_string(),
        r"\\?\UNC\server\share\x".to_string(),
        format!(r"\\?\Volume{{{GUID}}}\x"),
        r"\\.\C:\x".to_string(),
        format!(r"\\.\Volume{{{GUID}}}\x"),
    ] {
        assert!(is_absolute_path(&p), "must be absolute: {p}");
    }
}

#[test]
fn a_drive_root_is_absolute() {
    assert!(is_absolute_path(r"C:\"));
    assert!(is_absolute_path(r"D:\"));
}

#[test]
fn rejects_relative_drive_relative_root_relative_and_empty() {
    for p in ["", "a.txt", r"rel\b.txt", "C:rel", "C:", r"\root\c", r"\x", ".", ".."] {
        assert!(!is_absolute_path(p), "must reject {p:?}");
    }
}

#[test]
fn rejects_device_opening_forms_and_a_drive_without_trailing_backslash() {
    for p in [r"\\.\PhysicalDrive0", r"\\.\pipe\p", r"\\.\COM1", r"\\.\C:", r"\\.\", r"\\.\NUL"] {
        assert!(!is_absolute_path(p), "must reject {p}");
    }
}

// G-9: only ASCII letters are drive letters.
#[test]
fn drive_letters_are_ascii_only() {
    for p in [r"ｃ:\x", r"1:\x", r"é:\x", r":\x", r"_:\x"] {
        assert!(!is_absolute_path(p), "must reject {p}");
    }
    assert!(is_absolute_path(r"c:\x") && is_absolute_path(r"Z:\x"));
}

// The backend GUID check is strict (8-4-4-4-12 hex) for `\\.\Volume{..}`; the path bar is looser (AC-758 excludes it).
#[test]
fn the_device_namespace_volume_guid_must_be_a_strict_guid_with_a_trailing_backslash() {
    assert!(!is_absolute_path(r"\\.\Volume{abc}\x"));
    assert!(!is_absolute_path(&format!(r"\\.\Volume{{{GUID}}}")), "no trailing `\\` names the device itself");
    assert!(is_absolute_path(&format!(r"\\.\Volume{{{GUID}}}\")));
    // `\\?\Volume{..}` is just a `\\?\` path: accepted whatever follows.
    assert!(is_absolute_path(r"\\?\Volume{abc}\x"));
}

#[test]
fn unc_needs_a_non_empty_server_and_a_non_empty_share() {
    for p in [r"\\server", r"\\server\", r"\\\share", r"\\\", r"\\"] {
        assert!(!is_absolute_path(p), "must reject {p}");
    }
    assert!(is_absolute_path(r"\\server\share"));
}

// ---- length (UTF-16 units) ------------------------------------------------------------------------

#[test]
fn exactly_32767_utf16_units_is_absolute_and_32768_is_not() {
    let ok = format!(r"C:\{}", "a".repeat(32_767 - 3));
    let over = format!(r"C:\{}", "a".repeat(32_768 - 3));
    assert_eq!(utf16_len(&ok), 32_767);
    assert_eq!(utf16_len(&over), 32_768);
    assert!(is_absolute_path(&ok));
    assert!(!is_absolute_path(&over));
}

#[test]
fn surrogate_pairs_count_as_two_utf16_units() {
    let emoji = "😀"; // U+1F600 = 2 units
    let exact = format!(r"C:\{}", emoji.repeat((32_767 - 3) / 2)); // 3 + 16382 = 32767 units
    assert_eq!(utf16_len(&exact), 32_767);
    assert!(is_absolute_path(&exact));
    let over = format!("{exact}a");
    assert_eq!(utf16_len(&over), 32_768);
    assert!(!is_absolute_path(&over));
    assert!(over.chars().count() < 32_767, "by chars().count() it would still look short");
}

// ---- is_absolute_path_buf (resolved ZIP paths, T-13) ----------------------------------------------

#[test]
fn the_pathbuf_form_gives_the_same_answer_as_the_str_form() {
    for p in ac757_accepted().into_iter().chain(ac757_rejected()).chain(t12_accepted()) {
        assert_eq!(is_absolute_path_buf(std::path::Path::new(&p)), is_absolute_path(&p), "{:?}", p.chars().take(40).collect::<String>());
    }
}

#[cfg(windows)]
#[test]
fn a_non_utf8_pathbuf_is_not_absolute() {
    use std::os::windows::ffi::OsStringExt;
    // `C:\` + an unpaired surrogate: not valid Unicode.
    let wide: Vec<u16> = r"C:\".encode_utf16().chain([0xD800u16, b'x' as u16]).collect();
    let path = std::path::PathBuf::from(std::ffi::OsString::from_wide(&wide));
    assert!(path.to_str().is_none());
    assert!(!is_absolute_path_buf(&path));
}

// ---- first_relative (the path named in the R-89-2 message) -----------------------------------------

#[test]
fn first_relative_is_none_for_an_empty_input_and_for_all_absolute_paths() {
    assert_eq!(first_relative(Vec::<&str>::new()), None);
    assert_eq!(first_relative([r"C:\a", r"\\server\share\b"]), None);
}

#[test]
fn first_relative_returns_the_first_offender_in_order() {
    assert_eq!(first_relative([r"C:\a", "b.txt", "c.txt"]), Some("b.txt"));
    assert_eq!(first_relative(["", r"C:\a"]), Some(""));
    assert_eq!(first_relative([r"C:\a", "C:", r"\x"]), Some("C:"));
}

#[test]
fn first_relative_accepts_any_iterator_of_str() {
    let owned = vec![r"C:\a".to_string(), "rel".to_string()];
    assert_eq!(first_relative(owned.iter().map(String::as_str)), Some("rel"));
}

// ---- why `Path::is_absolute` is NOT used (T-01 alternative (a); documentation, passes today) --------

#[cfg(windows)]
#[test]
fn std_path_is_absolute_disagrees_with_the_spec_judgement_on_the_documented_values() {
    use std::path::Path;
    // `Path::is_absolute` says yes; R-722-3 says no.
    assert!(Path::new(r"\\.\pipe\x").is_absolute());
    assert!(Path::new("C:/foo").is_absolute());
    assert!(!is_absolute_path(r"\\.\pipe\x"));
    assert!(!is_absolute_path("C:/foo"));
    // M-5 (a): a UNC `\\server\share` without a trailing `\` is absolute for both.
    assert!(Path::new(r"\\server\share").is_absolute());
    assert!(is_absolute_path(r"\\server\share"));
}
