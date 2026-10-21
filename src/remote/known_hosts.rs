//! Trusted SFTP host keys (`%APPDATA%\tuif\known_hosts.toml`).
//!
//! Trust-on-first-use: an unknown key must be confirmed by the user before any
//! password is sent; a changed key is rejected during the handshake.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    Trusted,
    Unknown,
    Mismatch { expected: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHosts {
    /// `host:port` -> `"<algorithm> SHA256:<base64>"`
    #[serde(default)]
    pub hosts: BTreeMap<String, String>,
}

impl KnownHosts {
    pub fn check(&self, endpoint: &str, fingerprint: &str) -> HostKeyStatus {
        match self.hosts.get(endpoint) {
            None => HostKeyStatus::Unknown,
            Some(fp) if fp == fingerprint => HostKeyStatus::Trusted,
            Some(fp) => HostKeyStatus::Mismatch {
                expected: fp.clone(),
            },
        }
    }

    pub fn expected(&self, endpoint: &str) -> Option<String> {
        self.hosts.get(endpoint).cloned()
    }

    pub fn trust(&mut self, endpoint: &str, fingerprint: &str) {
        self.hosts
            .insert(endpoint.to_string(), fingerprint.to_string());
    }

    pub fn forget(&mut self, endpoint: &str) {
        self.hosts.remove(endpoint);
    }
}

pub fn load(path: &Path) -> KnownHosts {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, known: &KnownHosts) -> io::Result<()> {
    let text = toml::to_string_pretty(known)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    crate::config::atomic::atomic_write(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_trust_and_round_trip() {
        let mut k = KnownHosts::default();
        assert_eq!(k.check("h:22", "ssh-ed25519 SHA256:aaa"), HostKeyStatus::Unknown);
        k.trust("h:22", "ssh-ed25519 SHA256:aaa");
        assert_eq!(k.check("h:22", "ssh-ed25519 SHA256:aaa"), HostKeyStatus::Trusted);
        assert_eq!(
            k.check("h:22", "ssh-ed25519 SHA256:bbb"),
            HostKeyStatus::Mismatch {
                expected: "ssh-ed25519 SHA256:aaa".to_string()
            }
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts.toml");
        save(&path, &k).unwrap();
        assert_eq!(load(&path), k);
        k.forget("h:22");
        assert_eq!(k.check("h:22", "x"), HostKeyStatus::Unknown);
    }
}
