//! Remote (SFTP) file management — WinSCP-like integration (Phase 1).
//!
//! - [`profile`]: connection profiles (`connections.toml`)
//! - [`credential`]: saved passwords in the Windows Credential Manager
//! - [`known_hosts`]: trust-on-first-use host key store
//! - [`session`]: synchronous SFTP session (russh + russh-sftp)
//! - [`path`]: POSIX path helpers

pub mod credential;
pub mod known_hosts;
pub mod path;
pub mod profile;
pub mod session;

pub use known_hosts::{HostKeyStatus, KnownHosts};
pub use profile::ConnectionProfile;
pub use session::{handshake, PendingHandshake, RemoteSession, TransferStats};
