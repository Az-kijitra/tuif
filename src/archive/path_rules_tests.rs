//! TDD (unit-test-writer, 2026-09-27): `archive::path_rules` — entry-name
//! validation (extraction) and the create-side name helpers.
//!
//! Specs: SPEC-0300-0014 R-361, R-363 #1-#11, R-364 / R-364-1 / R-364-2 (fold
//! key), R-324, R-325 (surrogate row), AC-363, AC-364, AC-364-1, AC-364-2,
//! AC-364-3, AC-366, AC-378; SPEC-0200-0001 §3.4.9-a/b, §3.4.13-6,
//! SHOULD-90 / SHOULD-91 / SHOULD-96.
//!
//! NOT pinned (the spec is silent): whether a component such as `a..b` (dots
//! inside a name) is accepted; only a component equal to `..` is tested.

use super::error::ArchiveErrorCode;
use super::path_rules::*;

const REJECTED: ArchiveErrorCode = ArchiveErrorCode::ZipEntryRejected;

fn rejected(name: &str) {
    assert_eq!(validate_entry_name(name), Err(REJECTED), "expected {name:?} to be rejected");
}

fn components(name: &str) -> Vec<String> {
    validate_entry_name(name).unwrap_or_else(|e| panic!("{name:?} should be accepted, got {e:?}"))
}

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

// ------------------------------------------------------------ accepted names

#[test]
fn ordinary_names_are_accepted_and_split_into_components() {
    assert_eq!(components("a.txt"), s(&["a.txt"]));
    assert_eq!(components("dir/sub/file.txt"), s(&["dir", "sub", "file.txt"]));
    assert_eq!(components("日本語/ファイル.txt"), s(&["日本語", "ファイル.txt"]));
}

// R-361: `\` is a separator exactly like `/`.
#[test]
fn r361_backslash_is_a_separator() {
    assert_eq!(components(r"dir\sub\file.txt"), s(&["dir", "sub", "file.txt"]));
    assert_eq!(components(r"dir/sub\file.txt"), s(&["dir", "sub", "file.txt"]));
}

// AC-366 / R-363 note: `.` components are ignored, not rejected.
#[test]
fn ac366_dot_components_are_ignored() {
    assert_eq!(components("./a.txt"), s(&["a.txt"]));
    assert_eq!(components("a/./b.txt"), s(&["a", "b.txt"]));
    assert_eq!(components(r".\a.txt"), s(&["a.txt"]));
}

// SHOULD-91: empty components (`a//b`) are dropped, not rejected.
#[test]
fn should91_empty_components_are_dropped() {
    assert_eq!(components("a//b.txt"), s(&["a", "b.txt"]));
}

// A directory entry `d/` has the same components as `d`.
#[test]
fn directory_entries_with_a_trailing_slash_yield_the_same_components() {
    assert_eq!(components("d/"), s(&["d"]));
    assert_eq!(components("a/b/"), s(&["a", "b"]));
}

// Near-misses that must NOT be rejected (guards against over-blocking).
#[test]
fn names_that_only_look_like_reserved_names_are_accepted() {
    for name in ["CONSOLE", "COM0", "COM10", "LPT0", "AUXILIARY.txt", "nul_x.txt", "communication.txt", "CONIN", "c.on"] {
        assert!(validate_entry_name(name).is_ok(), "{name:?} must be accepted");
    }
}

#[test]
fn unusual_but_legal_characters_are_accepted() {
    for name in ["a b.txt", "a-b_c(1).txt", "#hash.txt", "a&b.txt", "a,b;c.txt", "sp ace/inner.txt", ".hidden", "a.b.c"] {
        assert!(validate_entry_name(name).is_ok(), "{name:?} must be accepted");
    }
}

// ---------------------------------------------- AC-363: traversal / absolute

// R-363 #1 (+ R-361 for the backslash forms).
#[test]
fn ac363_parent_directory_components_are_rejected() {
    for name in ["../evil.txt", "a/../../evil.txt", r"a\..\..\evil.txt", "..", "a/..", "a/../b"] {
        rejected(name);
    }
}

// R-363 #2: absolute / UNC / verbatim / device namespaces.
#[test]
fn ac363_absolute_paths_are_rejected() {
    for name in ["/abs.txt", r"\\server\share\x.txt", "//server/share/x", r"\\?\C:\x", r"\\.\COM1", r"\abs.txt"] {
        rejected(name);
    }
}

// R-363 #3.
#[test]
fn ac363_drive_specifications_are_rejected() {
    for name in ["C:/x.txt", "C:x.txt", r"C:\x.txt", "a/C:x"] {
        rejected(name);
    }
}

// ------------------------------------------------------ AC-364: reserved etc

// R-363 #4 (AC-364 list).
#[test]
fn ac364_reserved_device_names_are_rejected() {
    for name in [
        "CON", "nul.txt", "nul.tar.gz", "CON .txt", "dir/COM1.log", "LPT9", "CONIN$", "CONOUT$", "COM¹", "LPT³.txt",
        "PRN", "aux", "Con", "COM9.x.y", "COM²", "LPT¹",
    ] {
        rejected(name);
    }
}

#[test]
fn r363_4_is_reserved_device_name_uses_the_part_before_the_first_dot_and_ignores_case() {
    for name in ["CON", "con", "nul.tar.gz", "CON .txt", "COM1", "lpt9.log", "CONIN$", "CONOUT$.x", "COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"] {
        assert!(is_reserved_device_name(name), "{name:?}");
    }
    for name in ["CONSOLE", "COM0", "COM10", "LPT0", "a.con", "xcon", "nul_x", "", "COM"] {
        assert!(!is_reserved_device_name(name), "{name:?}");
    }
}

// R-363 #5.
#[test]
fn ac364_colon_and_alternate_data_streams_are_rejected() {
    for name in ["a.txt:stream", "a:b", "dir/a.txt:$DATA"] {
        rejected(name);
    }
}

// R-363 #6.
#[test]
fn ac364_trailing_dot_or_space_is_rejected() {
    for name in ["name.", "name ", "dir /x", "dir./x", "a/b.", "a/b ", "...", ". "] {
        rejected(name);
    }
}

// R-363 #7.
#[test]
fn ac364_forbidden_characters_are_rejected() {
    for name in ["a?b.txt", "a*b", "a<b", "a>b", "a\"b", "a|b", "a\u{0}b", "a\u{1f}b", "a\tb"] {
        rejected(name);
    }
}

// R-363 #8 / AC-364-1.
#[test]
fn ac364_1_empty_or_separator_only_names_are_rejected() {
    for name in ["", "/", "//", r"\", "./", "."] {
        rejected(name);
    }
}

// ---------------------------------------------------- R-363 #10: component len

// AC-364-2 (a): 256 UTF-16 units rejected, exactly 255 accepted.
#[test]
fn ac364_2_component_length_limit_is_255_utf16_units() {
    let ok = "a".repeat(MAX_COMPONENT_UTF16);
    let too_long = "a".repeat(MAX_COMPONENT_UTF16 + 1);
    assert!(validate_entry_name(&ok).is_ok());
    assert!(validate_entry_name(&format!("dir/{ok}")).is_ok());
    rejected(&too_long);
    rejected(&format!("dir/{too_long}/x.txt"));
}

// UTF-16 units, not chars or bytes: U+1F600 is 2 units.
#[test]
fn ac364_2_component_length_counts_utf16_code_units_not_chars() {
    let ok = "😀".repeat(127); // 254 units
    let edge = "😀".repeat(127) + "a"; // 255 units
    let too_long = "😀".repeat(128); // 256 units (only 128 chars)
    assert!(validate_entry_name(&ok).is_ok());
    assert!(validate_entry_name(&edge).is_ok());
    rejected(&too_long);
    // 255 multi-byte chars are 255 units but 765 bytes: accepted.
    assert!(validate_entry_name(&"あ".repeat(255)).is_ok());
}

// ------------------------------------------------ R-363 #9: full path length

#[test]
fn utf16_len_counts_code_units() {
    assert_eq!(utf16_len(""), 0);
    assert_eq!(utf16_len("abc"), 3);
    assert_eq!(utf16_len("あ"), 1);
    assert_eq!(utf16_len("😀"), 2);
}

#[test]
fn r363_9_limits_are_259_and_247() {
    assert_eq!(MAX_FILE_PATH_UTF16, 259);
    assert_eq!(MAX_DIR_PATH_UTF16, 247);
    assert_eq!(MAX_COMPONENT_UTF16, 255);
}

// AC-364-2 (b): destination `C:\x` (4 units) + `\` + name.
#[test]
fn ac364_2_file_path_may_be_259_units_but_not_260() {
    let dest = r"C:\x";
    let file_259 = vec!["f".repeat(259 - dest.len() - 1)]; // 4 + 1 + 254 = 259
    let file_260 = vec!["f".repeat(260 - dest.len() - 1)]; // 260 (component 255: legal on its own)
    assert_eq!(check_path_lengths(dest, &file_259, false), Ok(()));
    assert_eq!(check_path_lengths(dest, &file_260, false), Err(REJECTED));
}

// AC-364-2 (c): directories are limited to 247.
#[test]
fn ac364_2_directory_path_may_be_247_units_but_not_248() {
    let dest = r"C:\x";
    let dir_247 = vec!["d".repeat(247 - dest.len() - 1)];
    let dir_248 = vec!["d".repeat(248 - dest.len() - 1)];
    assert_eq!(check_path_lengths(dest, &dir_247, true), Ok(()));
    assert_eq!(check_path_lengths(dest, &dir_248, true), Err(REJECTED));
}

// SHOULD-90: the 247 limit also applies to IMPLIED parent directories even when
// the file itself is within 259.
#[test]
fn should90_implied_parent_directories_are_limited_to_247() {
    let dest = r"C:\x";
    // parent path = 4 + 1 + 243 = 248 (> 247); file path = 248 + 1 + 5 = 254 (< 259)
    let comps = vec!["p".repeat(243), "a.txt".to_string()];
    assert_eq!(check_path_lengths(dest, &comps, false), Err(REJECTED));
    // parent path exactly 247 -> accepted (file path 247 + 1 + 5 = 253).
    let ok = vec!["p".repeat(242), "a.txt".to_string()];
    assert_eq!(check_path_lengths(dest, &ok, false), Ok(()));
}

// The length is counted in UTF-16 units.
#[test]
fn r363_9_length_is_counted_in_utf16_units() {
    let dest = r"C:\x"; // 4
    // each 😀 = 2 units: 4 + 1 + 127*2 = 259 accepted; 4 + 1 + 127*2 + 1 = 260 rejected
    assert_eq!(check_path_lengths(dest, &["😀".repeat(127)], false), Ok(()));
    assert_eq!(check_path_lengths(dest, &[format!("{}a", "😀".repeat(127))], false), Err(REJECTED));
}

// SHOULD-90: a `\\?\` prefix is NOT counted.
#[test]
fn should90_verbatim_prefix_is_not_counted() {
    let plain = r"C:\x";
    let verbatim = r"\\?\C:\x";
    let comps = vec!["f".repeat(259 - plain.len() - 1)];
    assert_eq!(check_path_lengths(verbatim, &comps, false), Ok(()));
    let too_long = vec!["f".repeat(260 - plain.len() - 1)];
    assert_eq!(check_path_lengths(verbatim, &too_long, false), Err(REJECTED));
}

#[test]
fn should90_strip_verbatim_prefix() {
    assert_eq!(strip_verbatim_prefix(r"\\?\C:\x\y"), r"C:\x\y");
    assert_eq!(strip_verbatim_prefix(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
    assert_eq!(strip_verbatim_prefix(r"C:\plain"), r"C:\plain");
    assert_eq!(strip_verbatim_prefix(r"\\srv\share"), r"\\srv\share");
}

// UNC destinations count as `\\srv\share\...`.
#[test]
fn should90_unc_verbatim_destination_counts_as_a_plain_unc_path() {
    let plain = r"\\srv\share"; // 11
    let verbatim = r"\\?\UNC\srv\share";
    let comps = vec!["f".repeat(259 - plain.len() - 1)];
    assert_eq!(check_path_lengths(plain, &comps, false), Ok(()));
    assert_eq!(check_path_lengths(verbatim, &comps, false), Ok(()));
    let too_long = vec!["f".repeat(260 - plain.len() - 1)];
    assert_eq!(check_path_lengths(verbatim, &too_long, false), Err(REJECTED));
}

// A destination that is a drive root (`C:\`) must not double the separator.
#[test]
fn drive_root_destination_does_not_double_the_separator() {
    // `C:\` is 3 units; two components keep every component <= 255.
    let ok = vec!["d".repeat(100), "f".repeat(259 - 3 - 100 - 1)];
    assert_eq!(check_path_lengths(r"C:\", &ok, false), Ok(()));
    let too_long = vec!["d".repeat(100), "f".repeat(260 - 3 - 100 - 1)];
    assert_eq!(check_path_lengths(r"C:\", &too_long, false), Err(REJECTED));
}

// ---------------------------------------------- R-363 #11: unpaired surrogate

#[test]
fn r363_11_has_unpaired_surrogate() {
    assert!(has_unpaired_surrogate(&[0xD800]));
    assert!(has_unpaired_surrogate(&[0xDC00]));
    assert!(has_unpaired_surrogate(&[0xDC00, 0xD800])); // low then high
    assert!(has_unpaired_surrogate(&[b'a' as u16, 0xDBFF]));
    assert!(has_unpaired_surrogate(&[0xD83D, b'a' as u16])); // high not followed by low
    assert!(!has_unpaired_surrogate(&[0xD83D, 0xDE00])); // U+1F600
    assert!(!has_unpaired_surrogate(&[]));
    assert!(!has_unpaired_surrogate(&"日本語 abc 😀".encode_utf16().collect::<Vec<_>>()));
}

// R-325 last row / SHOULD-96 / AC-364-3 (create side): OS names.
#[cfg(windows)]
#[test]
fn should96_os_names_with_unpaired_surrogates_are_invalid_path() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let bad = OsString::from_wide(&[b'a' as u16, 0xD800, b'.' as u16, b't' as u16]);
    assert_eq!(os_name_to_utf8(&bad), Err(ArchiveErrorCode::InvalidPath));
    let good = OsString::from_wide(&"日本語😀.txt".encode_utf16().collect::<Vec<_>>());
    assert_eq!(os_name_to_utf8(&good), Ok("日本語😀.txt".to_string()));
}

#[test]
fn os_names_that_are_valid_unicode_convert_unchanged() {
    assert_eq!(os_name_to_utf8(std::ffi::OsStr::new("plain.txt")), Ok("plain.txt".to_string()));
}

// ------------------------------------------------------- R-364 fold key

// R-364 / AC-378: names differing only in letter case share a key.
#[test]
fn r364_case_only_differences_share_a_fold_key() {
    assert_eq!(fold_key(&s(&["A.txt"])), fold_key(&s(&["a.txt"])));
    assert_eq!(fold_key(&s(&["Dir", "x.TXT"])), fold_key(&s(&["dir", "X.txt"])));
    assert_eq!(fold_key(&s(&["Ärger"])), fold_key(&s(&["ärger"])));
    assert_eq!(fold_key(&s(&["ファイル", "ａ.txt"])), fold_key(&s(&["ファイル", "Ａ.txt"]))); // full-width a/A
}

#[test]
fn r364_different_names_have_different_fold_keys() {
    assert_ne!(fold_key(&s(&["a.txt"])), fold_key(&s(&["b.txt"])));
    assert_ne!(fold_key(&s(&["a", "b"])), fold_key(&s(&["ab"])), "component boundaries matter");
    assert_ne!(fold_key(&s(&["a", "b"])), fold_key(&s(&["a", "b", "c"])));
}

// §3.4.9-b: only a 1:1 uppercase fold; a char whose uppercase is several chars
// (ß -> SS) is kept as is; no Unicode normalization (NFC vs NFD differ).
#[test]
fn r364_fold_is_a_simple_one_to_one_uppercase_without_normalization() {
    assert_ne!(fold_key(&s(&["ß"])), fold_key(&s(&["SS"])));
    assert_ne!(fold_key(&s(&["\u{e9}"])), fold_key(&s(&["e\u{301}"])), "é (NFC) and e+U+0301 (NFD) stay distinct");
}

// SHOULD-91: `.` and empty components do not affect the key (they are dropped
// by `validate_entry_name`), so all these spell one entry.
#[test]
fn should91_spelling_variants_of_one_path_share_one_key() {
    let keys: Vec<String> =
        ["a/b.txt", "a//b.txt", "a/./b.txt", "./a/b.txt", r"a\b.txt", "A/B.TXT"].iter().map(|n| fold_key(&components(n))).collect();
    assert!(keys.windows(2).all(|w| w[0] == w[1]), "{keys:?}");
}

// --------------------------------------------- R-324: output file name rules

#[test]
fn r324_output_file_name_rules() {
    assert_eq!(validate_output_file_name("out.zip"), Ok(()));
    assert_eq!(validate_output_file_name("日本語 (1).zip"), Ok(()));
    for bad in ["", "   ", ".", "..", "a?.zip", "a*b", "a:b", "a\"b", "a<b", "a>b", "a|b", "a/b", r"a\b"] {
        assert_eq!(validate_output_file_name(bad), Err(ArchiveErrorCode::InvalidName), "{bad:?}");
    }
}
