//! `config/default_config.rs`: the commented `config.toml` template that `tuif --config` and
//! `tuif --init-config` (called by `install.ps1`) write when no config file exists yet.
//!
//! This is the single source of the template. It only ever CREATES a missing file; an existing
//! `config.toml` is never overwritten, renamed, or modified (the app reads it through
//! `config_toml::load` only).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// The template shipped inside the binary.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("default_config.toml");

/// Writes [`DEFAULT_CONFIG_TOML`] to `path` if nothing exists there yet.
///
/// Returns `Ok(true)` when the file was created, `Ok(false)` when a file already existed.
pub fn write_if_missing(path: &Path) -> io::Result<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // `create_new` makes the existence check and the creation one atomic step.
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(DEFAULT_CONFIG_TOML.as_bytes())?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::config_toml::{self, IconTheme};

    #[test]
    fn template_loads_without_errors_and_matches_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(write_if_missing(&path).unwrap());

        let loaded = config_toml::load(&path);
        assert_eq!(loaded.keymap.parse_error, None);
        assert!(loaded.keymap.entries.is_empty());
        assert!(loaded.external_apps.is_empty());
        assert_eq!(loaded.general, config_toml::GeneralConfig::default());
        assert_eq!(loaded.layout, config_toml::LayoutConfig::default());
        assert_eq!(loaded.layout.icons, IconTheme::Nerd);
    }

    #[test]
    fn template_only_uses_keys_the_loader_reads() {
        // Guards against the template advertising settings that are silently ignored.
        let value: toml::Value = DEFAULT_CONFIG_TOML.parse().unwrap();
        let table = value.as_table().unwrap();
        let known_top = [
            "theme",
            "general",
            "layout",
            "colors",
            "keymap",
            "external_apps",
        ];
        for key in table.keys() {
            assert!(
                known_top.contains(&key.as_str()),
                "unknown top-level key: {key}"
            );
        }
        let known_general = [
            "confirm_delete",
            "delete_to_trash",
            "dirs_first",
            "natural_sort",
            "scroll_offset",
        ];
        for key in table["general"].as_table().unwrap().keys() {
            assert!(
                known_general.contains(&key.as_str()),
                "unknown [general] key: {key}"
            );
        }
        let known_layout = [
            "tree_width_percent",
            "min_col_width",
            "time_format",
            "icons",
        ];
        for key in table["layout"].as_table().unwrap().keys() {
            assert!(
                known_layout.contains(&key.as_str()),
                "unknown [layout] key: {key}"
            );
        }
    }

    #[test]
    fn existing_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "theme = \"nord\"\n").unwrap();
        assert!(!write_if_missing(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "theme = \"nord\"\n");
    }

    #[test]
    fn creates_missing_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tuif").join("config.toml");
        assert!(write_if_missing(&path).unwrap());
        assert!(path.exists());
    }
}
