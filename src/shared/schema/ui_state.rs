//! Persisted display-state schema (SPEC-0300-0009 §7.1 R-77/R-77-1;
//! SPEC-0200-0001 §3.6.1/§3.6.11.3, MUST-87). One `ui-state.toml`, sibling of
//! `navigation.toml` / `viewer-window.toml` (same config directory).
//!
//! `config/ui_state.rs` does NOT use `#[derive(Deserialize)]` to parse this
//! type directly from disk (a single malformed field would otherwise fail
//! the whole table with `derive`'s all-or-nothing deserialization); it reads
//! a `toml::Value` and pulls each field out individually so that ONE bad
//! field falls back to its own default without discarding the rest
//! (AC-629 (iii)). `Deserialize` is still derived here for convenience
//! (round-trip tests, and any future strict-parse caller), but the
//! resilient path lives entirely in `config::ui_state::load`.

use serde::{Deserialize, Serialize};

/// R-77-1: the six fields `ui-state.toml` persists. Nothing else (sort-menu
/// open state, tree expansion, selection/focus, search) is saved (R-77-1
/// last sentence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiStateData {
    /// R-74: hidden-file visibility. Default false (not shown).
    #[serde(default = "default_ui_show_hidden")]
    pub ui_show_hidden: bool,
    /// R-21: size column. Default true (shown).
    #[serde(default = "default_ui_show_size")]
    pub ui_show_size: bool,
    /// R-21: modified-time column. Default false (hidden).
    #[serde(default = "default_ui_show_time")]
    pub ui_show_time: bool,
    /// R-13: tree panel. Default true (shown).
    #[serde(default = "default_ui_show_tree")]
    pub ui_show_tree: bool,
    /// R-30: one of "name" / "size" / "type" / "modified". Default "name".
    #[serde(default = "default_sort_key")]
    pub sort_key: String,
    /// R-30/R-31: "asc" / "desc". Default "asc".
    #[serde(default = "default_sort_order")]
    pub sort_order: String,
}

fn default_ui_show_hidden() -> bool {
    false
}
fn default_ui_show_size() -> bool {
    true
}
fn default_ui_show_time() -> bool {
    false
}
fn default_ui_show_tree() -> bool {
    true
}
fn default_sort_key() -> String {
    "name".to_string()
}
fn default_sort_order() -> String {
    "asc".to_string()
}

impl Default for UiStateData {
    fn default() -> Self {
        Self {
            ui_show_hidden: default_ui_show_hidden(),
            ui_show_size: default_ui_show_size(),
            ui_show_time: default_ui_show_time(),
            ui_show_tree: default_ui_show_tree(),
            sort_key: default_sort_key(),
            sort_order: default_sort_order(),
        }
    }
}

/// R-30: the only four valid sort keys (mirrors TS `SortKey`,
/// `features/file-list/sort.ts::SORT_KEYS`). Used by `config::ui_state::load`
/// to fall back an out-of-range `sort_key` to the default (AC-629 (iii)).
pub const KNOWN_SORT_KEYS: &[&str] = &["name", "size", "type", "modified"];
/// R-31: the only two valid sort orders.
pub const KNOWN_SORT_ORDERS: &[&str] = &["asc", "desc"];

#[cfg(test)]
mod tests {
    use super::*;

    // R-13/R-21/R-30/R-74: the exact documented default values.
    #[test]
    fn default_matches_the_documented_values() {
        let d = UiStateData::default();
        assert_eq!(
            d,
            UiStateData {
                ui_show_hidden: false,
                ui_show_size: true,
                ui_show_time: false,
                ui_show_tree: true,
                sort_key: "name".to_string(),
                sort_order: "asc".to_string(),
            }
        );
    }

    // R-77: all six fields round-trip through TOML under their documented
    // snake_case names.
    #[test]
    fn toml_round_trip_uses_the_six_documented_keys() {
        let d = UiStateData {
            ui_show_hidden: true,
            ui_show_size: false,
            ui_show_time: true,
            ui_show_tree: false,
            sort_key: "size".to_string(),
            sort_order: "desc".to_string(),
        };
        let s = toml::to_string_pretty(&d).expect("serialize");
        for key in ["ui_show_hidden", "ui_show_size", "ui_show_time", "ui_show_tree", "sort_key", "sort_order"] {
            assert!(s.contains(key), "missing {key} in: {s}");
        }
        let back: UiStateData = toml::from_str(&s).expect("parse");
        assert_eq!(back, d);
    }

    // Omitted fields default (the strict-derive path only -- the resilient
    // per-field fallback for INVALID values lives in config::ui_state::load).
    #[test]
    fn missing_fields_default_via_derive() {
        let d: UiStateData = toml::from_str("sort_key = \"size\"").expect("parse");
        assert_eq!(d.sort_key, "size");
        assert!(!d.ui_show_hidden);
        assert!(d.ui_show_size);
        assert!(!d.ui_show_time);
        assert!(d.ui_show_tree);
        assert_eq!(d.sort_order, "asc");
    }

    #[test]
    fn known_sort_keys_and_orders_match_the_frontend_set() {
        assert_eq!(KNOWN_SORT_KEYS, &["name", "size", "type", "modified"]);
        assert_eq!(KNOWN_SORT_ORDERS, &["asc", "desc"]);
    }
}
