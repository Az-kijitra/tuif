//! `config/config_toml.rs` (renamed from `keymap.rs`, SPEC-0200-0001 §3.11.3 H-1): reads `config.toml`
//! at startup, ONCE, in ONE TOML parse: the `input_keymap_custom` table (SPEC-0300-0010 R-45;
//! §3.6.11.3, MUST-86 "recommendation A") and the top-level `ui_theme` (SPEC-0300-0017 R-917).
//! This module hands back each keymap entry's RAW value plus a structural parse error (with a 1-based
//! line number when one can be recovered) and the RAW `ui_theme`; it performs NO per-element validation:
//! no known-action-id check (R-45-4 (i)), no key-string grammar check (R-45-4-1), no `delete`+Shift
//! rejection (R-18-2) -- all of that lives in the TypeScript
//! `features/keyboard/keymap-validate.ts::validateKeymapEntries` (§6.1: `config/` may not own
//! key-processing logic; the previous design that put this validation in Rust was withdrawn by
//! architecture-guardian MUST-86 because it re-implemented the grammar a second time and drifted from
//! the actual TS-side matcher, see docs/architecture/proposals/SPEC-0200-0001.md §3.6.11.3). Judging the
//! words of `ui_theme` is `shared::schema::ui_theme::interpret_ui_theme`'s job (R-917-3).
//!
//! `config/config_toml.rs` is also the ONLY reader of `config.toml` (MUST-92 as rewritten by H-1):
//! there is no `save`/write path for `config.toml` anywhere in this crate (`config/io.rs`, the former
//! read/write module, was deleted). AC-63/AC-630/AC-960 ("app operations never change config.toml's
//! bytes or mtime") are therefore checked against THIS module's `load` alone.
//!
//! ```ignore
//! pub fn load(path: &Path) -> ConfigTomlLoadResult
//! ```
//!
//! - Missing file (`NotFound`) => the default result: no entries, no error, `ui_theme: Missing`
//!   (R-45-1 / R-917-5 (1): no notice, no file is created).
//! - The whole file cannot be read (a TOML syntax error, non-UTF-8 bytes, or any other I/O error such as
//!   a permission error) => `keymap.parse_error: Some { whole_file: true }` and `ui_theme: Missing`
//!   (R-917-5 (2); the file is NEVER renamed, backed up, or rewritten -- unlike
//!   `config::navigation`/`config::ui_state`, there is no `.bak` recovery here, because `config.toml` is
//!   a human-edited, read-only-to-the-app file). `line` is `None` when no byte offset is available
//!   (SHOULD-121).
//! - The file parses as TOML but `input_keymap_custom` is not a table => `entries: []`,
//!   `parse_error: Some { whole_file: false }`, and `ui_theme` is read as usual (R-917-5 (3), 確定 #16).
//! - Otherwise every key of the table becomes one `RawKeymapEntry`; `value` is `Some(s)` when the TOML
//!   value is a string, `None` for any other type (AC-62 (iv) / MUST-86 (iv), e.g. `sort_menu = 123`).

use std::fs;
use std::io;
use std::path::Path;

use crate::shared::schema::ui_theme::UiThemeRaw;

/// One raw `input_keymap_custom` table entry, before any validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawKeymapEntry {
    pub action_id: String,
    /// `None` when the TOML value exists but is not a string (AC-62 (iv)).
    pub value: Option<String>,
}

/// R-45-2: a structural failure. `line` is the 1-based line number derived from the underlying
/// `toml::de::Error`'s byte-offset span, when one is available. `whole_file` tells whether the WHOLE
/// file could not be read (R-917-5 (2): the notice also says the theme fell back to the default) or
/// only `input_keymap_custom` had the wrong type (R-917-5 (3): the theme is unaffected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapParseError {
    pub message: String,
    pub line: Option<u32>,
    pub whole_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeymapRawLoadResult {
    pub entries: Vec<RawKeymapEntry>,
    /// `Some` only for a failure (R-45-2/R-45-3); never set together with a non-empty `entries`.
    pub parse_error: Option<KeymapParseError>,
}

/// Everything read from `config.toml` in one go (R-917-8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTomlLoadResult {
    pub keymap: KeymapRawLoadResult,
    pub ui_theme: UiThemeRaw,
}

impl Default for ConfigTomlLoadResult {
    fn default() -> Self {
        Self { keymap: KeymapRawLoadResult::default(), ui_theme: UiThemeRaw::Missing }
    }
}

/// R-917-5 (2): the whole file is unreadable; the theme falls back with the keymap.
fn whole_file_failure(message: String, line: Option<u32>) -> ConfigTomlLoadResult {
    ConfigTomlLoadResult {
        keymap: KeymapRawLoadResult {
            entries: Vec::new(),
            parse_error: Some(KeymapParseError { message, line, whole_file: true }),
        },
        ui_theme: UiThemeRaw::Missing,
    }
}

/// Recovers a 1-based line number from a `toml::de::Error`'s byte-offset span
/// (SHOULD-121 / R-45-2), when one is available.
fn line_number_from_span(err: &toml::de::Error, text: &str) -> Option<u32> {
    let span = err.span()?;
    let offset = span.start.min(text.len());
    Some(text[..offset].matches('\n').count() as u32 + 1)
}

/// The top-level `ui_theme` as it is, unjudged (R-917-3).
fn raw_ui_theme(table: &toml::value::Table) -> UiThemeRaw {
    match table.get("ui_theme") {
        None => UiThemeRaw::Missing,
        Some(toml::Value::String(s)) => UiThemeRaw::Str(s.clone()),
        Some(_) => UiThemeRaw::WrongType,
    }
}

pub fn load(path: &Path) -> ConfigTomlLoadResult {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return ConfigTomlLoadResult::default(),
        Err(e) => return whole_file_failure(e.to_string(), None),
    };

    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => return whole_file_failure(e.to_string(), None),
    };
    // A leading UTF-8 BOM (a legacy Notepad artifact) must not make an
    // otherwise-valid file look corrupt.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let value: toml::Value = match text.parse() {
        Ok(v) => v,
        Err(e) => return whole_file_failure(e.to_string(), line_number_from_span(&e, text)),
    };

    let Some(table) = value.as_table() else { return ConfigTomlLoadResult::default() };
    let ui_theme = raw_ui_theme(table);

    let keymap = match table.get("input_keymap_custom") {
        None => KeymapRawLoadResult::default(),
        Some(toml::Value::Table(entries_table)) => {
            let entries = entries_table
                .iter()
                .map(|(action_id, value)| RawKeymapEntry {
                    action_id: action_id.clone(),
                    value: value.as_str().map(|s| s.to_string()),
                })
                .collect();
            KeymapRawLoadResult { entries, parse_error: None }
        }
        Some(_) => KeymapRawLoadResult {
            entries: Vec::new(),
            parse_error: Some(KeymapParseError {
                message: "input_keymap_custom must be a table".to_string(),
                line: None,
                whole_file: false,
            }),
        },
    };
    ConfigTomlLoadResult { keymap, ui_theme }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut entries: Vec<RawKeymapEntry>) -> Vec<RawKeymapEntry> {
        entries.sort_by(|a, b| a.action_id.cmp(&b.action_id));
        entries
    }

    // R-45-1/AC-61 (i): a missing config.toml is normal -- empty entries, no
    // error, and (implicitly) no file is created by `load` itself.
    #[test]
    fn missing_file_returns_empty_entries_and_no_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let r = load(&path).keymap;

        assert_eq!(r.entries, Vec::new());
        assert!(r.parse_error.is_none());
        assert!(!path.exists(), "load must never create config.toml");
    }

    // R-45-3/AC-61 (ii): a TOML syntax error never renames/backs up/rewrites
    // the file -- unlike navigation.rs/ui_state.rs there is no `.bak` here.
    #[test]
    fn syntax_error_yields_a_parse_error_and_leaves_the_file_byte_for_byte_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let bad = "this is = = not toml [[[";
        fs::write(&path, bad).unwrap();

        let r = load(&path).keymap;

        assert_eq!(r.entries, Vec::new());
        assert!(r.parse_error.is_some());
        assert_eq!(fs::read_to_string(&path).unwrap(), bad, "config.toml must be byte-for-byte unchanged");
        assert!(!dir.path().join("config.toml.bak").exists(), "no backup file is ever created for config.toml");
    }

    // R-45-2: a line number is recovered for a syntax error later in the file.
    #[test]
    fn syntax_error_reports_a_1_based_line_number_when_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // Line 1 is valid; line 3 breaks (unterminated table header).
        fs::write(&path, "ui_show_hidden = false\n\n[input_keymap_custom\n").unwrap();

        let r = load(&path).keymap;

        let err = r.parse_error.expect("must be a parse error");
        assert!(err.line.is_some(), "a line number must be recoverable for this error: {err:?}");
        assert!(!err.message.is_empty());
    }

    // R-45-4 (ii)/MUST-86: `input_keymap_custom` present but not a table is a
    // whole-file failure, distinct from a per-element issue.
    #[test]
    fn input_keymap_custom_that_is_not_a_table_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "input_keymap_custom = \"not a table\"\n").unwrap();

        let r = load(&path).keymap;

        assert_eq!(r.entries, Vec::new());
        assert!(r.parse_error.is_some());
    }

    // R-45: a config.toml with no input_keymap_custom table at all is fine
    // (empty overrides, no error) -- comments/unrelated settings are ignored.
    #[test]
    fn a_config_with_no_input_keymap_custom_table_yields_empty_entries_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "# a comment\nsome_future_setting = 1\n").unwrap();

        let r = load(&path).keymap;

        assert_eq!(r.entries, Vec::new());
        assert!(r.parse_error.is_none());
    }

    // R-45: every key of the table becomes one raw entry, string values kept
    // verbatim (no grammar/known-id validation performed here, MUST-86).
    #[test]
    fn valid_string_entries_are_returned_verbatim_unvalidated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "ui_show_hidden = false\n\n[input_keymap_custom]\ntoggle_hidden = \"Ctrl+Alt+H\"\nundo = \"Ctrl+Z\"\n",
        )
        .unwrap();

        let r = load(&path).keymap;

        assert!(r.parse_error.is_none());
        assert_eq!(
            sorted(r.entries),
            sorted(vec![
                RawKeymapEntry { action_id: "toggle_hidden".into(), value: Some("Ctrl+Alt+H".into()) },
                // "undo" is not one of the 34 known action ids -- MUST-86: this
                // module does NOT filter it out, that is keymap-validate.ts's job.
                RawKeymapEntry { action_id: "undo".into(), value: Some("Ctrl+Z".into()) },
            ])
        );
    }

    // AC-62 (iv)/MUST-86 (iv): a non-string value (e.g. `sort_menu = 123`)
    // becomes `value: None`, not a whole-file failure.
    #[test]
    fn a_non_string_value_becomes_value_none_without_failing_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[input_keymap_custom]\nsort_menu = 123\ntoggle_time = \"Ctrl+Alt+T\"\n").unwrap();

        let r = load(&path).keymap;

        assert!(r.parse_error.is_none());
        assert_eq!(
            sorted(r.entries),
            sorted(vec![
                RawKeymapEntry { action_id: "sort_menu".into(), value: None },
                RawKeymapEntry { action_id: "toggle_time".into(), value: Some("Ctrl+Alt+T".into()) },
            ])
        );
    }

    // R-31: an empty string is a perfectly valid value (disables the action)
    // -- it is NOT the same as a non-string value, and must round-trip as
    // `Some("")`, not `None`.
    #[test]
    fn an_empty_string_value_is_kept_as_some_empty_string_not_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[input_keymap_custom]\nhelp_keymap = \"\"\n").unwrap();

        let r = load(&path).keymap;

        assert_eq!(r.entries, vec![RawKeymapEntry { action_id: "help_keymap".into(), value: Some(String::new()) }]);
    }

    // Non-UTF-8 bytes are a parse failure too (SHOULD-121), like navigation.rs.
    #[test]
    fn non_utf8_bytes_are_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let bytes: Vec<u8> = vec![0xff, 0xfe, 0x00, 0xc3, 0x28, 0xa0, 0xa1];
        fs::write(&path, &bytes).unwrap();

        let r = load(&path).keymap;

        assert_eq!(r.entries, Vec::new());
        assert!(r.parse_error.is_some());
        assert_eq!(fs::read(&path).unwrap(), bytes, "config.toml must be byte-for-byte unchanged");
    }

    // SHOULD-121: a non-NotFound I/O error (e.g. the path is a directory) is
    // also a parse error, with no line number available.
    #[test]
    fn a_non_notfound_io_error_is_a_parse_error_with_no_line_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::create_dir(&path).unwrap();

        let r = load(&path).keymap;

        let err = r.parse_error.expect("a directory at the path must be reported as a parse error");
        assert!(err.line.is_none());
    }

    // Regression (§19 第14回 "参考(非ブロッカー)": UTF-8 BOM'd config.toml, a
    // legacy Notepad artifact, must not be treated as corrupt.
    #[test]
    fn a_leading_utf8_bom_does_not_make_a_valid_file_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"[input_keymap_custom]\ntoggle_hidden = \"Ctrl+Alt+H\"\n");
        fs::write(&path, &bytes).unwrap();

        let r = load(&path).keymap;

        assert!(r.parse_error.is_none(), "a BOM-prefixed valid config.toml must still load: {r:?}");
        assert_eq!(r.entries, vec![RawKeymapEntry { action_id: "toggle_hidden".into(), value: Some("Ctrl+Alt+H".into()) }]);
    }

    // AC-63/AC-630 (MUST-92): `config.toml`'s content AND mtime are
    // byte-for-byte unchanged by loading it -- config/config_toml.rs is a
    // read-only path (R-77-4/R-53). Repeated loads must not touch the file
    // either.
    #[test]
    fn ac63_ac630_load_never_modifies_config_tomls_content_or_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original =
            "# a hand-edited config.toml\nui_show_hidden = false\n\n[input_keymap_custom]\ntoggle_hidden = \"Ctrl+Alt+H\"\n";
        fs::write(&path, original).unwrap();
        let mtime_before = fs::metadata(&path).unwrap().modified().unwrap();

        load(&path);
        load(&path);

        let mtime_after = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original, "content must be byte-for-byte unchanged");
        assert_eq!(mtime_before, mtime_after, "mtime must be unchanged (no write ever happens)");
        let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(entries.len(), 1, "load must never create a sibling file (no .bak, no .tmp)");
    }
}
