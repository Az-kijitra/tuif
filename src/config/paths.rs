//! Config directory resolution for `tuif` (`%APPDATA%\tuif\` on Windows).

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Name of the override environment variable (`TUIF_CONFIG_DIR`, `CUIFILER_CONFIG_DIR`, or `REFLEXFILES_CONFIG_DIR`).
const CONFIG_DIR_ENV: &str = "TUIF_CONFIG_DIR";
#[cfg(debug_assertions)]
const CUIFILER_CONFIG_DIR_ENV: &str = "CUIFILER_CONFIG_DIR";
#[cfg(debug_assertions)]
const LEGACY_CONFIG_DIR_ENV: &str = "REFLEXFILES_CONFIG_DIR";

/// Returns the app config directory (`%APPDATA%\tuif`), creating it if missing.
pub fn app_config_dir() -> io::Result<PathBuf> {
    if let Some(dir) = config_dir_override()? {
        return Ok(dir);
    }
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA environment variable is not set"))?;
    let dir = PathBuf::from(appdata).join("tuif");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Full path of `navigation.toml`.
pub fn navigation_path() -> io::Result<PathBuf> {
    Ok(app_config_dir()?.join("navigation.toml"))
}

/// Full path of `config.toml`.
pub fn config_path() -> io::Result<PathBuf> {
    Ok(app_config_dir()?.join("config.toml"))
}

/// Full path of `ui-state.toml`.
pub fn ui_state_path() -> io::Result<PathBuf> {
    Ok(app_config_dir()?.join("ui-state.toml"))
}

/// Full path of `connections.toml` (SFTP connection profiles; never contains passwords).
pub fn connections_path() -> io::Result<PathBuf> {
    Ok(app_config_dir()?.join("connections.toml"))
}

/// Full path of `known_hosts.toml` (trusted SFTP host key fingerprints).
pub fn known_hosts_path() -> io::Result<PathBuf> {
    Ok(app_config_dir()?.join("known_hosts.toml"))
}

#[derive(Debug)]
pub struct ConfigDirOverrideError(pub String);

impl fmt::Display for ConfigDirOverrideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{CONFIG_DIR_ENV}: {}", self.0)
    }
}

impl std::error::Error for ConfigDirOverrideError {}

impl From<ConfigDirOverrideError> for io::Error {
    fn from(e: ConfigDirOverrideError) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, e.to_string())
    }
}

pub fn resolve_config_dir_override(raw: Option<&OsStr>) -> Result<Option<PathBuf>, ConfigDirOverrideError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let text = raw
        .to_str()
        .ok_or_else(|| ConfigDirOverrideError("value is not valid Unicode".to_string()))?;
    if text.trim().is_empty() {
        return Err(ConfigDirOverrideError("value is empty".to_string()));
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err(ConfigDirOverrideError(format!("not an absolute path: {text}")));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ConfigDirOverrideError(format!("must not contain `..`: {text}")));
    }
    if path.exists() && !path.is_dir() {
        return Err(ConfigDirOverrideError(format!("not a directory: {text}")));
    }
    fs::create_dir_all(&path).map_err(|e| ConfigDirOverrideError(format!("cannot create {text}: {e}")))?;
    Ok(Some(path))
}

pub fn override_notice_line(dir: &Path) -> String {
    format!("[config] override dir: {}", dir.display())
}

#[cfg(debug_assertions)]
fn read_override_var() -> Option<OsString> {
    std::env::var_os(CONFIG_DIR_ENV)
        .or_else(|| std::env::var_os(CUIFILER_CONFIG_DIR_ENV))
        .or_else(|| std::env::var_os(LEGACY_CONFIG_DIR_ENV))
}

#[cfg(not(debug_assertions))]
fn read_override_var() -> Option<OsString> {
    None
}

#[cfg(debug_assertions)]
fn announce_override(dir: &Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| eprintln!("{}", override_notice_line(dir)));
}

#[cfg(not(debug_assertions))]
fn announce_override(_dir: &Path) {}

pub fn config_dir_override() -> io::Result<Option<PathBuf>> {
    let raw = read_override_var();
    let resolved = resolve_config_dir_override(raw.as_deref())?;
    if let Some(dir) = &resolved {
        announce_override(dir);
    }
    Ok(resolved)
}
