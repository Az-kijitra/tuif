//! Phase 12 tests (unit-test-writer, 2026-10-06): `interpret_ui_theme` (`shared/schema/ui_theme.rs`, no `tauri` reference).
//! Specs: SPEC-0300-0017 R-917 (1/2: fixed vs following the OS), R-917-3 (exact, case-sensitive match of
//! `"light"` / `"dark"` / `"system"`; no trimming), R-917-4 (an invalid value is reported), AC-950
//! (the `config.toml` axis of the table: missing key, the three words, `"Dark"` / `" dark"` / `1` / `true` /
//! `[]` / table); SPEC-0200-0001 §3.11.3, §3.11.8 (H-2: the OS axis is not part of this pure function).

use super::ui_theme::{interpret_ui_theme, ThemeResolution, ThemeSetting, UiThemeRaw};

fn s(v: &str) -> UiThemeRaw {
    UiThemeRaw::Str(v.to_string())
}

fn res(setting: ThemeSetting, invalid: bool) -> ThemeResolution {
    ThemeResolution { setting, invalid }
}

// R-917 (2): no key follows the OS, and that is not an error (R-45-1 style: nothing to report).
#[test]
fn a_missing_key_follows_the_os_without_a_notice() {
    assert_eq!(interpret_ui_theme(&UiThemeRaw::Missing), res(ThemeSetting::System, false));
}

#[test]
fn light_is_fixed_light() {
    assert_eq!(interpret_ui_theme(&s("light")), res(ThemeSetting::Light, false));
}

#[test]
fn dark_is_fixed_dark() {
    assert_eq!(interpret_ui_theme(&s("dark")), res(ThemeSetting::Dark, false));
}

// 確定 #7: an explicit "system" follows the OS and is valid (no notice).
#[test]
fn system_follows_the_os_without_a_notice() {
    assert_eq!(interpret_ui_theme(&s("system")), res(ThemeSetting::System, false));
}

// R-917-3 / 確定 #8 / AC-950: case differences are NOT normalised; the value is an invalid one.
#[test]
fn matching_is_case_sensitive() {
    for v in ["Dark", "DARK", "Light", "LIGHT", "System", "SYSTEM", "dArK"] {
        assert_eq!(interpret_ui_theme(&s(v)), res(ThemeSetting::System, true), "{v:?} must be invalid");
    }
}

// R-917-3 / AC-950 (`" dark"`): surrounding whitespace is NOT trimmed.
#[test]
fn surrounding_whitespace_is_not_trimmed() {
    for v in [" dark", "dark ", " dark ", "\tdark", "dark\n", " light", "light ", " system", "system "] {
        assert_eq!(interpret_ui_theme(&s(v)), res(ThemeSetting::System, true), "{v:?} must be invalid");
    }
}

// AC-950: an empty string and other words are invalid, never a fixed theme.
#[test]
fn an_empty_or_unknown_word_is_invalid_and_follows_the_os() {
    for v in ["", " ", "auto", "default", "dark-mode", "darker", "0", "true"] {
        assert_eq!(interpret_ui_theme(&s(v)), res(ThemeSetting::System, true), "{v:?} must be invalid");
    }
}

// Full-width / look-alike characters are not "dark" either (exact match, no Unicode folding).
#[test]
fn look_alike_characters_are_not_folded() {
    assert_eq!(interpret_ui_theme(&s("ｄａｒｋ")), res(ThemeSetting::System, true));
    assert_eq!(interpret_ui_theme(&s("dark\u{200b}")), res(ThemeSetting::System, true));
}

// AC-950: `1` / `true` / `[]` / a table all reach the function as `WrongType`.
#[test]
fn a_value_of_the_wrong_type_is_invalid_and_follows_the_os() {
    assert_eq!(interpret_ui_theme(&UiThemeRaw::WrongType), res(ThemeSetting::System, true));
}

// Every invalid result follows the OS (never a fixed theme): R-917-4.
#[test]
fn an_invalid_value_never_yields_a_fixed_theme() {
    for raw in [s("Dark"), s(" dark"), s(""), UiThemeRaw::WrongType] {
        let r = interpret_ui_theme(&raw);
        assert!(r.invalid, "{raw:?}");
        assert_eq!(r.setting, ThemeSetting::System, "{raw:?}");
    }
}

// `invalid` is true exactly when the value was present but not one of the three words.
#[test]
fn invalid_is_false_for_the_three_words_and_a_missing_key() {
    for raw in [UiThemeRaw::Missing, s("light"), s("dark"), s("system")] {
        assert!(!interpret_ui_theme(&raw).invalid, "{raw:?}");
    }
}

// A pure function: same input, same output, input untouched.
#[test]
fn the_function_is_pure() {
    let raw = s("dark");
    let before = raw.clone();
    assert_eq!(interpret_ui_theme(&raw), interpret_ui_theme(&raw));
    assert_eq!(raw, before);
}
