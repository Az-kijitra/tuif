//! Persisted navigation schema (SPEC-0300-0013 R-203/R-204/R-239, SPEC-0200-0001
//! §3.2.2). Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
//! pub struct NavigationData {
//!     #[serde(default)] pub visit_history: Vec<String>,
//!     #[serde(default)] pub jump_list: Vec<String>,
//!     #[serde(default)] pub search_history: Vec<String>,
//! }
//! ```

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NavigationData {
    #[serde(default)]
    pub visit_history: Vec<String>,
    #[serde(default)]
    pub jump_list: Vec<String>,
    #[serde(default)]
    pub search_history: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // R-203-1/R-204-1: plain string arrays only; default is three empty lists.
    #[test]
    fn default_is_three_empty_lists() {
        let d = NavigationData::default();
        assert!(d.visit_history.is_empty());
        assert!(d.jump_list.is_empty());
        assert!(d.search_history.is_empty());
    }

    // R-239: the three lists live in one TOML document under these keys.
    #[test]
    fn toml_round_trip_uses_the_three_documented_keys() {
        let d = NavigationData {
            visit_history: vec!["C:\\a".into()],
            jump_list: vec!["C:\\j".into()],
            search_history: vec!["q".into()],
        };
        let s = toml::to_string_pretty(&d).expect("serialize");
        assert!(s.contains("visit_history"));
        assert!(s.contains("jump_list"));
        assert!(s.contains("search_history"));
        let back: NavigationData = toml::from_str(&s).expect("parse");
        assert_eq!(back, d);
    }

    // R-242-1: omitted fields fall back to empty lists.
    #[test]
    fn missing_fields_default_to_empty() {
        let d: NavigationData = toml::from_str("jump_list = [\"C:\\\\j\"]").expect("parse");
        assert!(d.visit_history.is_empty());
        assert_eq!(d.jump_list, vec!["C:\\j".to_string()]);
        assert!(d.search_history.is_empty());
    }

    // R-239-2: legacy (ReflexFiles) field names are not aliased.
    #[test]
    fn legacy_field_names_are_not_read() {
        let d: NavigationData =
            toml::from_str("history_paths = [\"C:\\\\x\"]\nhistory_search = [\"q\"]")
                .expect("parse");
        assert!(d.visit_history.is_empty());
        assert!(d.search_history.is_empty());
    }
}
