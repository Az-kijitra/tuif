//! `ui-state.toml` load/save (SPEC-0300-0009 §7.1 R-77~R-77-6;
//! SPEC-0200-0001 §3.6.1, §3.6.11.3 MUST-87/90). Same shape as
//! `config::navigation` (IoLock + atomic_write, `.bak` recovery on structural
//! corruption) with ONE addition: tolerant, field-by-field extraction so a
//! single invalid field (AC-629 (iii), e.g. `ui_show_size = "yes"`) falls
//! back to just that field's default instead of discarding the whole file.
//! Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! pub struct UiStateLoadResult { pub data: UiStateData, pub recovered_from_corruption: bool }
//! pub fn load(path: &Path) -> io::Result<UiStateLoadResult>
//! pub fn save(path: &Path, data: &UiStateData) -> io::Result<()>
//! ```
//!
//! `load`:
//! - Missing file (`NotFound`) => `UiStateData::default()`,
//!   `recovered_from_corruption: false` (AC-629 (i), no notice).
//! - The file exists but is not valid UTF-8, OR does not parse as TOML AT
//!   ALL (`toml::Value` cannot be produced) => STRUCTURAL corruption: rename
//!   to `<path>.toml.bak` (overwriting), return the default,
//!   `recovered_from_corruption: true` (AC-629 (ii); the caller shows a
//!   status message, R-77-3).
//! - Any OTHER I/O error (e.g. permission denied) => `Err` (never silently
//!   treated as "missing", mirrors `config::navigation`'s
//!   `non_notfound_io_error_is_propagated_as_err`).
//! - Otherwise (a `toml::Value::Table` was produced): each of the six fields
//!   is extracted independently from the table (`Value::as_bool`/
//!   `Value::as_str`, NOT `#[derive(Deserialize)]` on the whole struct) --
//!   a missing or type-mismatched field falls back to ONLY that field's
//!   default (AC-629 (iii)); `sort_key`/`sort_order` additionally fall back
//!   to their default when the string is not one of
//!   `shared::schema::ui_state::KNOWN_SORT_KEYS`/`KNOWN_SORT_ORDERS`. This
//!   path never sets `recovered_from_corruption` and never notifies (R-77-3
//!   "個々の項目の値が不正な場合は…通知しない").
//!
//! `save`: always writes all six fields via `config::atomic::atomic_write`
//! (R-77-2/SHOULD-65); the caller is expected to serialize concurrent saves
//! through `UiStateIoLock` (`config::commands`, SHOULD-117), exactly like
//! `NavigationIoLock` around `config::navigation::save`.

use std::fs;
use std::io;
use std::path::Path;

use crate::shared::schema::ui_state::{UiStateData, KNOWN_SORT_KEYS, KNOWN_SORT_ORDERS};

pub struct UiStateLoadResult {
    pub data: UiStateData,
    /// R-77-3: true only for STRUCTURAL corruption (non-UTF-8 or unparsable
    /// TOML) that triggered a `.bak` rename -- never for a single bad field
    /// (AC-629 (iii), which is silent).
    pub recovered_from_corruption: bool,
}

fn extract_bool(table: &toml::value::Table, key: &str, default: bool) -> bool {
    table.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn extract_known_str(
    table: &toml::value::Table,
    key: &str,
    known: &[&str],
    default: &str,
) -> String {
    match table.get(key).and_then(|v| v.as_str()) {
        Some(s) if known.contains(&s) => s.to_string(),
        _ => default.to_string(),
    }
}

/// AC-629 (iii): each field is extracted independently from a `toml::Value`
/// table so a single invalid/missing field falls back to only its own
/// default -- never `#[derive(Deserialize)]` on the whole struct.
fn extract_ui_state_data(table: &toml::value::Table) -> UiStateData {
    let default = UiStateData::default();
    UiStateData {
        ui_show_hidden: extract_bool(table, "ui_show_hidden", default.ui_show_hidden),
        ui_show_size: extract_bool(table, "ui_show_size", default.ui_show_size),
        ui_show_time: extract_bool(table, "ui_show_time", default.ui_show_time),
        ui_show_tree: extract_bool(table, "ui_show_tree", default.ui_show_tree),
        sort_key: extract_known_str(table, "sort_key", KNOWN_SORT_KEYS, &default.sort_key),
        sort_order: extract_known_str(table, "sort_order", KNOWN_SORT_ORDERS, &default.sort_order),
    }
}

pub fn load(path: &Path) -> io::Result<UiStateLoadResult> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(UiStateLoadResult {
                data: UiStateData::default(),
                recovered_from_corruption: false,
            });
        }
        Err(e) => return Err(e),
    };

    let parsed = String::from_utf8(bytes)
        .ok()
        .and_then(|s| s.parse::<toml::Value>().ok());
    match parsed {
        Some(value) => {
            let table = value.as_table().cloned().unwrap_or_default();
            Ok(UiStateLoadResult {
                data: extract_ui_state_data(&table),
                recovered_from_corruption: false,
            })
        }
        None => {
            let backup_path = path.with_extension("toml.bak");
            fs::rename(path, &backup_path)?;
            Ok(UiStateLoadResult {
                data: UiStateData::default(),
                recovered_from_corruption: true,
            })
        }
    }
}

pub fn save(path: &Path, data: &UiStateData) -> io::Result<()> {
    let serialized =
        toml::to_string_pretty(data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::config::atomic::atomic_write(path, &serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // AC-629 (i): missing file is normal -- defaults, no backup, no error.
    #[test]
    fn missing_file_returns_defaults_without_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");

        let r = load(&path).expect("load");

        assert_eq!(r.data, UiStateData::default());
        assert!(!r.recovered_from_corruption);
        assert!(!dir.path().join("ui-state.toml.bak").exists());
    }

    // R-77: save then load round-trips every field.
    #[test]
    fn save_then_load_round_trips_all_six_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        let data = UiStateData {
            ui_show_hidden: true,
            ui_show_size: false,
            ui_show_time: true,
            ui_show_tree: false,
            sort_key: "size".to_string(),
            sort_order: "desc".to_string(),
        };

        save(&path, &data).expect("save");
        let r = load(&path).expect("load");

        assert_eq!(r.data, data);
        assert!(!r.recovered_from_corruption);
    }

    // R-77-2/AC-632: exactly one file, no leftover `.tmp` (atomic_write's contract).
    #[test]
    fn save_produces_a_single_file_and_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        save(&path, &UiStateData::default()).expect("save");
        let count = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 1);
        assert!(!dir.path().join("ui-state.toml.tmp").exists());
    }

    // AC-629 (ii): a TOML that cannot be parsed at all is structural
    // corruption -- backed up, defaults returned, recovery flagged.
    #[test]
    fn unparsable_toml_is_backed_up_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        let bad = "this is = = not toml [[[";
        fs::write(&path, bad).unwrap();

        let r = load(&path).expect("load");

        assert_eq!(r.data, UiStateData::default());
        assert!(r.recovered_from_corruption);
        assert_eq!(
            fs::read_to_string(dir.path().join("ui-state.toml.bak")).unwrap(),
            bad
        );
        assert!(
            !path.exists(),
            "the corrupt original must no longer sit at the load path"
        );
    }

    // Non-UTF-8 bytes are structural corruption too (MUST-48(a) precedent).
    #[test]
    fn non_utf8_bytes_are_backed_up_like_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        let bytes: Vec<u8> = vec![0xff, 0xfe, 0x00, 0xc3, 0x28, 0xa0, 0xa1];
        fs::write(&path, &bytes).unwrap();

        let r = load(&path).expect("non-UTF-8 must be recovered, not Err");

        assert_eq!(r.data, UiStateData::default());
        assert!(r.recovered_from_corruption);
        assert_eq!(
            fs::read(dir.path().join("ui-state.toml.bak")).unwrap(),
            bytes
        );
    }

    // A genuine I/O error (not NotFound) must never be papered over as
    // "missing, use defaults, ok to save" (same precedent as navigation.rs).
    #[test]
    fn non_notfound_io_error_is_propagated_as_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::create_dir(&path).unwrap();
        assert!(load(&path).is_err());
    }

    // If the `.bak` rename itself fails, load returns Err (the caller must
    // then not treat this session as successfully hydrated).
    #[test]
    fn backup_rename_failure_is_propagated_as_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::write(&path, "garbage [[[").unwrap();
        let bak = dir.path().join("ui-state.toml.bak");
        fs::create_dir(&bak).unwrap();
        fs::write(bak.join("keep"), "x").unwrap(); // non-empty dir: rename cannot replace it

        assert!(load(&path).is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "garbage [[[",
            "original must be left intact"
        );
    }

    // AC-629 (iii): ONE field with the wrong TOML type falls back to just
    // that field's default; every other field is still honored, and this is
    // NOT reported as a recovery (no notice, R-77-3).
    #[test]
    fn ac629_iii_a_single_invalid_field_falls_back_alone_without_recovery_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::write(
            &path,
            "ui_show_hidden = true\nui_show_size = \"yes\"\nui_show_time = true\nui_show_tree = false\nsort_key = \"size\"\nsort_order = \"desc\"\n",
        )
        .unwrap();

        let r = load(&path).expect("load");

        assert!(
            !r.recovered_from_corruption,
            "a single bad field is not a structural corruption"
        );
        assert_eq!(
            r.data,
            UiStateData {
                ui_show_hidden: true, // preserved
                ui_show_size: true,   // fell back to the default (was a string, not a bool)
                ui_show_time: true,   // preserved
                ui_show_tree: false,  // preserved
                sort_key: "size".to_string(),
                sort_order: "desc".to_string(),
            }
        );
    }

    // AC-629 (iii): an out-of-range sort_key falls back to the default alone.
    #[test]
    fn an_unknown_sort_key_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::write(&path, "sort_key = \"nonsense\"\n").unwrap();

        let r = load(&path).expect("load");

        assert_eq!(r.data.sort_key, "name");
        assert!(!r.recovered_from_corruption);
    }

    // AC-629 (iii): an out-of-range sort_order falls back to the default alone.
    #[test]
    fn an_unknown_sort_order_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::write(&path, "sort_order = \"sideways\"\n").unwrap();

        let r = load(&path).expect("load");

        assert_eq!(r.data.sort_order, "asc");
        assert!(!r.recovered_from_corruption);
    }

    // A field simply absent from the file behaves the same as an invalid
    // one -- its own default, everything else preserved.
    #[test]
    fn a_missing_field_defaults_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::write(&path, "ui_show_hidden = true\n").unwrap();

        let r = load(&path).expect("load");

        assert!(r.data.ui_show_hidden);
        assert!(r.data.ui_show_size); // default
        assert_eq!(r.data.sort_key, "name"); // default
    }

    // §3.2.2 precedent: an existing `.bak` is overwritten by a newer corruption.
    #[test]
    fn existing_bak_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.toml");
        fs::write(dir.path().join("ui-state.toml.bak"), "older").unwrap();
        fs::write(&path, "newer [[[").unwrap();
        let r = load(&path).expect("load");
        assert!(r.recovered_from_corruption);
        assert_eq!(
            fs::read_to_string(dir.path().join("ui-state.toml.bak")).unwrap(),
            "newer [[["
        );
    }
}
