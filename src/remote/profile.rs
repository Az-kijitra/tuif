//! SFTP connection profiles persisted in `%APPDATA%\tuif\connections.toml`.
//!
//! Passwords are never written here; when `save_password` is set they live in the
//! Windows Credential Manager under [`ConnectionProfile::credential_target`].

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    /// Stable identifier (survives renames; used for the credential target name).
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    /// Empty means "the login (home) directory".
    #[serde(default)]
    pub initial_dir: String,
    #[serde(default)]
    pub save_password: bool,
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 22,
            user: String::new(),
            initial_dir: String::new(),
            save_password: true,
        }
    }
}

impl ConnectionProfile {
    /// Label shown in the tree / connection manager.
    pub fn display_label(&self) -> String {
        let port = if self.port == 22 {
            String::new()
        } else {
            format!(":{}", self.port)
        };
        format!("{} ({}@{}{})", self.name, self.user, self.host, port)
    }

    /// Windows Credential Manager target name for the saved password.
    pub fn credential_target(&self) -> String {
        format!("tuif:sftp:{}", self.id)
    }

    /// Key used to look up the trusted host key (`host:port`, host lowercased).
    pub fn endpoint_key(&self) -> String {
        format!("{}:{}", self.host.trim().to_ascii_lowercase(), self.port)
    }

    /// `sftp://user@host[:port]` prefix used in the path bar.
    pub fn url_prefix(&self) -> String {
        let port = if self.port == 22 {
            String::new()
        } else {
            format!(":{}", self.port)
        };
        format!("sftp://{}@{}{}", self.user, self.host, port)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ConnectionsFile {
    #[serde(default, rename = "connection")]
    connections: Vec<ConnectionProfile>,
}

/// Loads profiles. A missing file yields an empty list; a malformed file is an error.
pub fn load(path: &Path) -> Result<Vec<ConnectionProfile>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    toml::from_str::<ConnectionsFile>(&text)
        .map(|f| f.connections)
        .map_err(|e| format!("Invalid {}: {e}", path.display()))
}

pub fn save(path: &Path, profiles: &[ConnectionProfile]) -> io::Result<()> {
    let file = ConnectionsFile {
        connections: profiles.to_vec(),
    };
    let text = toml::to_string_pretty(&file)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    crate::config::atomic::atomic_write(path, &text)
}

/// Generates a unique, filesystem/credential-safe id from a display name.
pub fn generate_id(name: &str, existing: &[ConnectionProfile]) -> String {
    let mut slug: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-').to_string();
    let base = if slug.is_empty() {
        "conn".to_string()
    } else {
        slug
    };
    if !existing.iter().any(|p| p.id == base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|cand| !existing.iter().any(|p| &p.id == cand))
        .unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> ConnectionProfile {
        ConnectionProfile {
            id: id.to_string(),
            name: "Dev Server".to_string(),
            host: "192.168.1.10".to_string(),
            port: 22,
            user: "toshi".to_string(),
            initial_dir: "/home/toshi".to_string(),
            save_password: true,
        }
    }

    #[test]
    fn round_trips_through_toml_and_never_contains_a_password_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connections.toml");
        let profiles = vec![sample("dev-server")];
        save(&path, &profiles).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.lines().any(|l| l.trim_start().starts_with("password")));
        assert_eq!(load(&path).unwrap(), profiles);
    }

    #[test]
    fn missing_file_is_empty_and_port_defaults_to_22() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("none.toml")).unwrap().is_empty());
        let path = dir.path().join("c.toml");
        std::fs::write(
            &path,
            "[[connection]]\nid='a'\nname='a'\nhost='h'\nuser='u'\n",
        )
        .unwrap();
        let p = &load(&path).unwrap()[0];
        assert_eq!(p.port, 22);
        assert!(!p.save_password);
        assert_eq!(p.initial_dir, "");
    }

    #[test]
    fn generate_id_slugifies_and_deduplicates() {
        let existing = vec![sample("dev-server")];
        assert_eq!(generate_id("Prod  Box!", &existing), "prod-box");
        assert_eq!(generate_id("Dev Server", &existing), "dev-server-2");
        assert_eq!(generate_id("日本語", &existing), "conn");
    }

    #[test]
    fn labels_and_keys() {
        let mut p = sample("x");
        assert_eq!(p.display_label(), "Dev Server (toshi@192.168.1.10)");
        assert_eq!(p.url_prefix(), "sftp://toshi@192.168.1.10");
        p.port = 2222;
        p.host = "Example.COM".to_string();
        assert_eq!(p.endpoint_key(), "example.com:2222");
        assert_eq!(p.credential_target(), "tuif:sftp:x");
    }
}
