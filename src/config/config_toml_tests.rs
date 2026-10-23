//! Phase 12 tests (unit-test-writer, 2026-10-06): `config/config_toml.rs` (renamed from `keymap.rs`) reads
//! `input_keymap_custom` AND `ui_theme` from ONE read / ONE TOML parse.
//! Specs: SPEC-0300-0017 R-917-2 / R-917-5 (3 cases: file missing / whole file unreadable /
//! `input_keymap_custom` of the wrong type with a valid `ui_theme`, 確定 #16), R-917-6 / R-917-8 (read-only,
//! one read), AC-950, AC-959, AC-960; SPEC-0300-0010 R-45, AC-63; SPEC-0300-0009 AC-630;
//! SPEC-0200-0001 §3.11.3 (H-1), §3.11.12 (2).
//!
//! `ui_theme` is handed back RAW (`Missing` / `Str(s)` / `WrongType`); judging the words is
//! `interpret_ui_theme`'s job. Whole file unreadable (read error, not UTF-8, TOML syntax error):
//! `keymap.parse_error = Some { whole_file: true }` and `ui_theme = Missing`. `input_keymap_custom` present but not a
//! table: `Some { whole_file: false }` and `ui_theme` interpreted normally. The pre-existing inline tests of the old
//! `keymap.rs` live in `config_toml.rs` (called as `load(&p).keymap`) and are not duplicated here.

use std::fs;
use std::path::PathBuf;

use super::config_toml::{load, KeymapParseError, RawKeymapEntry};
use crate::shared::schema::ui_theme::UiThemeRaw;

fn write_config(contents: impl AsRef<[u8]>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, contents).unwrap();
    (dir, path)
}

fn s(v: &str) -> UiThemeRaw {
    UiThemeRaw::Str(v.to_string())
}

// ------------------------------------------------------------------ ui_theme extraction

// R-917-5 case 1: the file does not exist -> defaults, nothing to report, nothing created.
#[test]
fn a_missing_file_yields_defaults_with_ui_theme_missing_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let r = load(&path);

    assert_eq!(r.keymap.entries, Vec::new());
    assert!(r.keymap.parse_error.is_none());
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
    assert!(!path.exists(), "load must never create config.toml");
}

#[test]
fn a_file_without_ui_theme_has_ui_theme_missing() {
    let (_d, path) = write_config("# comment\nsome_future_setting = 1\n");
    let r = load(&path);
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
    assert!(r.keymap.parse_error.is_none());
}

// AC-950: the strings are returned verbatim (case and whitespace kept).
#[test]
fn a_string_ui_theme_is_returned_verbatim() {
    for v in ["light", "dark", "system", "Dark", " dark", "dark ", ""] {
        let (_d, path) = write_config(format!("ui_theme = {v:?}\n"));
        let r = load(&path);
        assert_eq!(r.ui_theme, s(v), "value {v:?}");
        assert!(r.keymap.parse_error.is_none(), "value {v:?}");
    }
}

// AC-950: `1` / `true` / `[]` / a table (and other TOML types) are `WrongType`, never an error of the file.
#[test]
fn a_non_string_ui_theme_is_wrong_type() {
    for body in [
        "ui_theme = 1\n",
        "ui_theme = 1.5\n",
        "ui_theme = true\n",
        "ui_theme = []\n",
        "ui_theme = [\"dark\"]\n",
        "ui_theme = { mode = \"dark\" }\n",
        "ui_theme = 1979-05-27T07:32:00Z\n",
        "[ui_theme]\nmode = \"dark\"\n",
    ] {
        let (_d, path) = write_config(body);
        let r = load(&path);
        assert_eq!(r.ui_theme, UiThemeRaw::WrongType, "body {body:?}");
        assert!(
            r.keymap.parse_error.is_none(),
            "a wrong-typed ui_theme is not a whole-file failure: {body:?}"
        );
    }
}

// Only the TOP-LEVEL key counts: `ui_theme` inside another table is that table's entry.
#[test]
fn ui_theme_inside_another_table_is_not_the_theme_setting() {
    let (_d, path) = write_config("[input_keymap_custom]\nui_theme = \"Ctrl+Alt+T\"\n");
    let r = load(&path);
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
    assert_eq!(
        r.keymap.entries,
        vec![RawKeymapEntry {
            action_id: "ui_theme".into(),
            value: Some("Ctrl+Alt+T".into())
        }]
    );
}

// A leading BOM (Notepad) must not hide the theme either.
#[test]
fn a_bom_prefixed_file_still_yields_ui_theme() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"ui_theme = \"dark\"\n");
    let (_d, path) = write_config(bytes);
    let r = load(&path);
    assert_eq!(r.ui_theme, s("dark"));
    assert!(r.keymap.parse_error.is_none());
}

// ------------------------------------------------------------------ one read, both settings (R-917-8)

#[test]
fn one_load_returns_both_the_keymap_entries_and_the_ui_theme() {
    let (_d, path) = write_config(
        "ui_theme = \"dark\"\n\n[input_keymap_custom]\ntoggle_time = \"Ctrl+Alt+T\"\nsort_menu = 123\n",
    );

    let r = load(&path);

    assert_eq!(r.ui_theme, s("dark"));
    assert!(r.keymap.parse_error.is_none());
    let mut entries = r.keymap.entries.clone();
    entries.sort_by(|a, b| a.action_id.cmp(&b.action_id));
    assert_eq!(
        entries,
        vec![
            RawKeymapEntry {
                action_id: "sort_menu".into(),
                value: None
            },
            RawKeymapEntry {
                action_id: "toggle_time".into(),
                value: Some("Ctrl+Alt+T".into())
            },
        ]
    );
}

// AC-958: an invalid ui_theme does not disturb the (valid) keymap of the same file.
#[test]
fn an_invalid_ui_theme_leaves_the_keymap_entries_intact() {
    let (_d, path) = write_config(
        "ui_theme = \"Dark\"\n\n[input_keymap_custom]\ntoggle_time = \"Ctrl+Alt+T\"\n",
    );
    let r = load(&path);
    assert_eq!(r.ui_theme, s("Dark"));
    assert!(r.keymap.parse_error.is_none());
    assert_eq!(
        r.keymap.entries,
        vec![RawKeymapEntry {
            action_id: "toggle_time".into(),
            value: Some("Ctrl+Alt+T".into())
        }]
    );
}

// ------------------------------------------------------------------ R-917-5 case 2: whole file unreadable

fn whole_file_error(r: &super::config_toml::ConfigTomlLoadResult) -> &KeymapParseError {
    r.keymap
        .parse_error
        .as_ref()
        .expect("a whole-file failure must be reported")
}

// AC-959 (ii): a TOML syntax error -> whole_file, and the theme falls back with it (Missing), even if a
// well-formed `ui_theme` line precedes the error.
#[test]
fn a_syntax_error_is_a_whole_file_failure_and_ui_theme_is_missing() {
    let (_d, path) = write_config("ui_theme = \"dark\"\nthis is = = not toml [[[\n");
    let r = load(&path);
    let err = whole_file_error(&r);
    assert!(err.whole_file);
    assert!(
        err.line.is_some(),
        "the line number is still recovered (R-45-2)"
    );
    assert_eq!(r.keymap.entries, Vec::new());
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
}

#[test]
fn non_utf8_bytes_are_a_whole_file_failure_and_ui_theme_is_missing() {
    let (_d, path) = write_config(vec![0xff, 0xfe, 0x00, 0xc3, 0x28, 0xa0, 0xa1]);
    let r = load(&path);
    assert!(whole_file_error(&r).whole_file);
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
}

// A non-NotFound I/O error (the path is a directory) is also "the whole file cannot be read".
#[test]
fn a_read_error_is_a_whole_file_failure_and_ui_theme_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::create_dir(&path).unwrap();
    let r = load(&path);
    let err = whole_file_error(&r);
    assert!(err.whole_file);
    assert!(err.line.is_none());
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
}

// ------------------------------------------------------------------ R-917-5 case 3 (確定 #16)

// `input_keymap_custom` is not a table, but the file IS valid TOML: whole_file is false and ui_theme is
// interpreted as usual (so `ui_theme = "dark"` still applies).
#[test]
fn input_keymap_custom_of_the_wrong_type_keeps_a_valid_ui_theme() {
    let (_d, path) = write_config("ui_theme = \"dark\"\ninput_keymap_custom = \"Ctrl+X\"\n");
    let r = load(&path);
    let err = whole_file_error(&r);
    assert!(
        !err.whole_file,
        "R-917-5 case 3: the R-45-2 notice must not mention the theme"
    );
    assert_eq!(r.keymap.entries, Vec::new());
    assert_eq!(r.ui_theme, s("dark"));
}

#[test]
fn input_keymap_custom_of_the_wrong_type_with_an_invalid_ui_theme_reports_the_theme_value_too() {
    let (_d, path) = write_config("ui_theme = 1\ninput_keymap_custom = [\"x\"]\n");
    let r = load(&path);
    assert!(!whole_file_error(&r).whole_file);
    assert_eq!(r.ui_theme, UiThemeRaw::WrongType);
}

#[test]
fn input_keymap_custom_of_the_wrong_type_without_ui_theme_has_ui_theme_missing() {
    let (_d, path) = write_config("input_keymap_custom = 5\n");
    let r = load(&path);
    assert!(!whole_file_error(&r).whole_file);
    assert_eq!(r.ui_theme, UiThemeRaw::Missing);
}

// A healthy file never reports a parse error (so `whole_file` can never be misread as set).
#[test]
fn a_healthy_file_has_no_parse_error() {
    let (_d, path) = write_config(
        "ui_theme = \"light\"\n[input_keymap_custom]\ntoggle_hidden = \"Ctrl+Alt+H\"\n",
    );
    assert!(load(&path).keymap.parse_error.is_none());
}

// ------------------------------------------------------------------ AC-63 / AC-630 / AC-960: read-only

// The same read-only guarantee as before, with `ui_theme` in the content (hand-edited comments included).
#[test]
fn ac63_ac630_ac960_load_never_modifies_content_or_mtime_with_ui_theme_present() {
    let original = "# hand-edited\nui_theme = \"dark\"   # my theme\nui_show_hidden = false\n\n[input_keymap_custom]\ntoggle_hidden = \"Ctrl+Alt+H\"\n";
    let (dir, path) = write_config(original);
    let mtime_before = fs::metadata(&path).unwrap().modified().unwrap();

    load(&path);
    load(&path);

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original,
        "content (comments included) must be byte-for-byte unchanged"
    );
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        mtime_before,
        "mtime must be unchanged"
    );
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "no sibling file (.bak / .tmp) may appear"
    );
}

// AC-960 / R-45-3: the failure paths do not touch the file either.
#[test]
fn ac63_load_leaves_a_broken_or_wrong_typed_file_untouched() {
    for body in [
        "ui_theme = \"dark\"\nthis is = = not toml [[[",
        "ui_theme = \"dark\"\ninput_keymap_custom = \"x\"\n",
        "ui_theme = 1\n",
    ] {
        let (dir, path) = write_config(body);
        let mtime_before = fs::metadata(&path).unwrap().modified().unwrap();

        load(&path);

        assert_eq!(fs::read_to_string(&path).unwrap(), body);
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            mtime_before
        );
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no .bak is ever created for config.toml"
        );
    }
}
