//! Synchronous SFTP session wrapper over `russh` + `russh-sftp`.
//!
//! `tuif` is a synchronous TUI, so every remote operation runs to completion via
//! `block_on` on a shared background Tokio runtime. The runtime's worker thread
//! keeps the SSH connection alive (keepalives, window updates) between calls.
//!
//! Connection is split into two steps so the UI can confirm an unknown host key
//! *before* any password is sent:
//! 1. [`handshake`] — TCP + SSH key exchange; returns the server fingerprint.
//! 2. [`PendingHandshake::authenticate`] — password (falls back to keyboard-interactive),
//!    then opens the `sftp` subsystem.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use russh::client::{self, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;

use super::known_hosts::{HostKeyStatus, KnownHosts};
use super::path as rpath;
use super::profile::ConnectionProfile;
use crate::fs::query::{Entry, EntryType};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);
/// Per-request SFTP timeout (seconds).
const SFTP_REQUEST_TIMEOUT_SECS: u64 = 15;
const CHUNK: usize = 64 * 1024;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("tuif-sftp")
            .enable_all()
            .build()
            .expect("failed to start SFTP runtime")
    })
}

struct ClientHandler {
    observed: Arc<Mutex<Option<String>>>,
    expected: Option<String>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fp = fingerprint_of(server_public_key);
        if let Ok(mut slot) = self.observed.lock() {
            *slot = Some(fp.clone());
        }
        // Unknown keys are accepted here only to finish key exchange; the caller must
        // obtain user confirmation before authenticating. Changed keys abort the handshake.
        Ok(self.expected.as_ref().is_none_or(|e| e == &fp))
    }
}

fn fingerprint_of(key: &PublicKeyOrCertificate) -> String {
    let pk = key.public_key();
    format!(
        "{} {}",
        pk.algorithm().as_str(),
        pk.fingerprint(HashAlg::Sha256)
    )
}

/// Result of step 1 (key exchange done, not yet authenticated).
pub struct PendingHandshake {
    handle: Handle<ClientHandler>,
    pub profile: ConnectionProfile,
    pub fingerprint: String,
    pub status: HostKeyStatus,
}

impl std::fmt::Debug for PendingHandshake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingHandshake")
            .field("profile", &self.profile.id)
            .field("fingerprint", &self.fingerprint)
            .field("status", &self.status)
            .finish()
    }
}

/// Step 1: TCP connect + SSH key exchange (no credentials are sent).
pub fn handshake(
    profile: &ConnectionProfile,
    known: &KnownHosts,
) -> Result<PendingHandshake, String> {
    let endpoint = profile.endpoint_key();
    let expected = known.expected(&endpoint);
    let observed = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        observed: observed.clone(),
        expected: expected.clone(),
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let host = profile.host.trim().to_string();
    let port = profile.port;
    let res = runtime().block_on(async move {
        tokio::time::timeout(
            CONNECT_TIMEOUT,
            client::connect(config, (host.as_str(), port), handler),
        )
        .await
    });
    let seen = observed.lock().ok().and_then(|s| s.clone());
    let handle = match res {
        Err(_) => {
            return Err(format!(
                "Connection to {} timed out",
                profile.endpoint_key()
            ))
        }
        Ok(Err(e)) => {
            if let (Some(exp), Some(got)) = (&expected, &seen) {
                if exp != got {
                    return Err(format!(
                        "HOST KEY CHANGED for {endpoint}! expected {exp}, got {got}. Connection refused (possible MITM). Remove the entry from known_hosts.toml if the change is legitimate."
                    ));
                }
            }
            return Err(format!("Cannot connect to {endpoint}: {e}"));
        }
        Ok(Ok(h)) => h,
    };
    let fingerprint = seen.unwrap_or_default();
    let status = known.check(&endpoint, &fingerprint);
    Ok(PendingHandshake {
        handle,
        profile: profile.clone(),
        fingerprint,
        status,
    })
}

impl PendingHandshake {
    /// Step 2: authenticate with a password (keyboard-interactive fallback) and open SFTP.
    pub fn authenticate(self, password: &str) -> Result<RemoteSession, String> {
        let PendingHandshake {
            mut handle,
            profile,
            ..
        } = self;
        let user = profile.user.clone();
        let pw = password.to_string();
        let rt = runtime();
        let sftp = rt.block_on(async {
            tokio::time::timeout(AUTH_TIMEOUT, async {
                let mut ok = handle
                    .authenticate_password(user.clone(), pw.clone())
                    .await
                    .map_err(|e| format!("Authentication error: {e}"))?
                    .success();
                if !ok {
                    ok = keyboard_interactive(&mut handle, &user, &pw).await?;
                }
                if !ok {
                    return Err("Authentication failed (wrong user name or password)".to_string());
                }
                let channel = handle
                    .channel_open_session()
                    .await
                    .map_err(|e| format!("Cannot open SSH channel: {e}"))?;
                channel
                    .request_subsystem(true, "sftp")
                    .await
                    .map_err(|e| format!("SFTP subsystem unavailable: {e}"))?;
                let sftp = SftpSession::new(channel.into_stream())
                    .await
                    .map_err(|e| format!("SFTP init failed: {e}"))?;
                sftp.set_timeout(SFTP_REQUEST_TIMEOUT_SECS);
                Ok::<_, String>(sftp)
            })
            .await
            .map_err(|_| "Authentication timed out".to_string())?
        })?;
        let home = rt
            .block_on(sftp.canonicalize("."))
            .map(|p| rpath::normalize(&p))
            .unwrap_or_else(|_| "/".to_string());
        Ok(RemoteSession {
            handle,
            sftp,
            home,
            profile,
        })
    }
}

async fn keyboard_interactive(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    password: &str,
) -> Result<bool, String> {
    let mut resp = match handle
        .authenticate_keyboard_interactive_start(user.to_string(), None::<String>)
        .await
    {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };
    for _ in 0..5 {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts.iter().map(|_| password.to_string()).collect();
                resp = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await
                    .map_err(|e| format!("Authentication error: {e}"))?;
            }
        }
    }
    Ok(false)
}

/// Outcome counters for batch transfers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TransferStats {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
}

/// Real-time progress update for an individual file during transfer.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub file_name: String,
    pub file_bytes_transferred: u64,
    pub file_total_bytes: u64,
    pub completed_files: usize,
    pub total_bytes_transferred: u64,
}

/// User's decision when a target file already exists during transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    Overwrite,
    Skip,
    OverwriteAll,
    SkipAll,
    Cancel,
}

struct TransferContext<'a, F, C> {
    pub cancel: &'a AtomicBool,
    pub on_progress: &'a mut F,
    pub on_conflict: &'a mut C,
}

/// An authenticated SFTP session.
pub struct RemoteSession {
    handle: Handle<ClientHandler>,
    sftp: SftpSession,
    /// Login directory (canonicalized `.`).
    pub home: String,
    pub profile: ConnectionProfile,
}

impl std::fmt::Debug for RemoteSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteSession")
            .field("profile", &self.profile.id)
            .field("home", &self.home)
            .finish()
    }
}

fn err(op: &str, path: &str, e: impl std::fmt::Display) -> String {
    format!("{op} failed for {path}: {e}")
}

fn mtime_string(secs: Option<u32>) -> String {
    secs.map(|s| format!("{:020}", s)).unwrap_or_default()
}

fn ext_of(name: &str, is_dir: bool) -> String {
    if is_dir {
        return String::new();
    }
    match name.rfind('.') {
        Some(idx) if idx > 0 => name[idx + 1..].to_string(),
        _ => String::new(),
    }
}

impl RemoteSession {
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Initial directory for this connection (profile setting, else home).
    pub fn initial_dir(&self) -> String {
        let d = self.profile.initial_dir.trim();
        if d.is_empty() {
            self.home.clone()
        } else if d.starts_with('/') {
            rpath::normalize(d)
        } else {
            rpath::normalize(&rpath::join(&self.home, d))
        }
    }

    pub fn canonicalize(&self, path: &str) -> Result<String, String> {
        runtime()
            .block_on(self.sftp.canonicalize(path))
            .map(|p| rpath::normalize(&p))
            .map_err(|e| err("Resolve path", path, e))
    }

    pub fn is_dir(&self, path: &str) -> Result<bool, String> {
        runtime()
            .block_on(self.sftp.metadata(path))
            .map(|m| m.is_dir())
            .map_err(|e| err("Stat", path, e))
    }

    pub fn exists(&self, path: &str) -> bool {
        runtime()
            .block_on(self.sftp.try_exists(path))
            .unwrap_or(false)
    }

    /// Lists `path` as file-list entries (symlinks are resolved to their target type).
    /// `Entry::path` holds the POSIX remote path.
    pub fn list_dir(&self, path: &str, show_hidden: bool) -> Result<Vec<Entry>, String> {
        let dir = rpath::normalize(path);
        runtime().block_on(async {
            let rd = self
                .sftp
                .read_dir(dir.clone())
                .await
                .map_err(|e| err("List", &dir, e))?;
            let mut out = Vec::new();
            for de in rd {
                let name = de.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let hidden = name.starts_with('.');
                if hidden && !show_hidden {
                    continue;
                }
                let full = rpath::join(&dir, &name);
                let mut meta = de.metadata();
                if meta.file_type().is_symlink() {
                    if let Ok(target) = self.sftp.metadata(full.clone()).await {
                        meta = target;
                    }
                }
                let is_dir = meta.is_dir();
                out.push(Entry {
                    ext: ext_of(&name, is_dir),
                    path: PathBuf::from(&full),
                    entry_type: if is_dir {
                        EntryType::Dir
                    } else {
                        EntryType::File
                    },
                    size: if is_dir { 0 } else { meta.size.unwrap_or(0) },
                    modified: mtime_string(meta.mtime),
                    hidden,
                    name,
                });
            }
            Ok(out)
        })
    }

    /// Child directories of `path` as `(name, posix_path)`, sorted case-insensitively.
    pub fn list_child_dirs(
        &self,
        path: &str,
        show_hidden: bool,
    ) -> Result<Vec<(String, String)>, String> {
        let mut dirs: Vec<(String, String)> = self
            .list_dir(path, show_hidden)?
            .into_iter()
            .filter(|e| e.entry_type == EntryType::Dir)
            .map(|e| (e.name, e.path.to_string_lossy().into_owned()))
            .collect();
        dirs.sort_by_key(|a| a.0.to_lowercase());
        Ok(dirs)
    }

    pub fn create_dir(&self, path: &str) -> Result<(), String> {
        runtime()
            .block_on(self.sftp.create_dir(path))
            .map_err(|e| err("Create folder", path, e))
    }

    /// Creates an empty file; fails if it already exists.
    pub fn create_file(&self, path: &str) -> Result<(), String> {
        runtime().block_on(async {
            let mut f = self
                .sftp
                .open_with_flags(
                    path,
                    OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
                )
                .await
                .map_err(|e| err("Create file", path, e))?;
            f.shutdown().await.map_err(|e| err("Create file", path, e))
        })
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        runtime()
            .block_on(self.sftp.rename(from, to))
            .map_err(|e| err("Rename", from, e))
    }

    /// Deletes a file, symlink, or directory tree (permanently — SFTP has no trash).
    pub fn remove_recursive(&self, path: &str) -> Result<(), String> {
        runtime().block_on(async {
            let meta = self
                .sftp
                .symlink_metadata(path)
                .await
                .map_err(|e| err("Delete", path, e))?;
            if !meta.is_dir() {
                return self
                    .sftp
                    .remove_file(path)
                    .await
                    .map_err(|e| err("Delete", path, e));
            }
            // Post-order traversal with an explicit stack (no async recursion).
            let mut stack = vec![(rpath::normalize(path), false)];
            while let Some((dir, visited)) = stack.pop() {
                if visited {
                    self.sftp
                        .remove_dir(dir.clone())
                        .await
                        .map_err(|e| err("Delete folder", &dir, e))?;
                    continue;
                }
                stack.push((dir.clone(), true));
                let rd = self
                    .sftp
                    .read_dir(dir.clone())
                    .await
                    .map_err(|e| err("List", &dir, e))?;
                for de in rd {
                    let name = de.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let child = rpath::join(&dir, &name);
                    if de.file_type().is_dir() {
                        stack.push((child, false));
                    } else {
                        self.sftp
                            .remove_file(child.clone())
                            .await
                            .map_err(|e| err("Delete", &child, e))?;
                    }
                }
            }
            Ok(())
        })
    }

    /// Downloads `remote` (file or directory tree) to exactly `local_dest`.
    pub fn download(&self, remote: &str, local_dest: &Path) -> Result<TransferStats, String> {
        let cancel = AtomicBool::new(false);
        self.download_with_progress(
            remote,
            local_dest,
            &cancel,
            |_| {},
            |_| ConflictChoice::Overwrite,
        )
    }

    /// Downloads `remote` to `local_dest` with cancellation, conflict resolution, and progress reporting.
    pub fn download_with_progress<F, C>(
        &self,
        remote: &str,
        local_dest: &Path,
        cancel: &AtomicBool,
        mut on_progress: F,
        mut on_conflict: C,
    ) -> Result<TransferStats, String>
    where
        F: FnMut(&ProgressUpdate),
        C: FnMut(&str) -> ConflictChoice,
    {
        runtime().block_on(async {
            let mut stats = TransferStats::default();
            let mut cx = TransferContext {
                cancel,
                on_progress: &mut on_progress,
                on_conflict: &mut on_conflict,
            };
            let meta = self
                .sftp
                .metadata(remote)
                .await
                .map_err(|e| err("Stat", remote, e))?;
            if !meta.is_dir() {
                self.download_file_with_progress(
                    remote,
                    local_dest,
                    meta.size.unwrap_or(0),
                    meta.mtime,
                    &mut stats,
                    &mut cx,
                )
                .await?;
                return Ok(stats);
            }
            let mut stack = vec![(rpath::normalize(remote), local_dest.to_path_buf())];
            while let Some((rdir, ldir)) = stack.pop() {
                if cx.cancel.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".to_string());
                }
                std::fs::create_dir_all(&ldir)
                    .map_err(|e| err("Create folder", &ldir.display().to_string(), e))?;
                stats.dirs += 1;
                let rd = self
                    .sftp
                    .read_dir(rdir.clone())
                    .await
                    .map_err(|e| err("List", &rdir, e))?;
                for de in rd {
                    if cx.cancel.load(Ordering::Relaxed) {
                        return Err("Transfer cancelled by user".to_string());
                    }
                    let name = de.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let rchild = rpath::join(&rdir, &name);
                    let lchild = ldir.join(sanitize_windows_name(&name));
                    let mut m = de.metadata();
                    if m.file_type().is_symlink() {
                        match self.sftp.metadata(rchild.clone()).await {
                            Ok(t) => m = t,
                            Err(_) => continue, // dangling link
                        }
                    }
                    if m.is_dir() {
                        stack.push((rchild, lchild));
                    } else {
                        self.download_file_with_progress(
                            &rchild,
                            &lchild,
                            m.size.unwrap_or(0),
                            m.mtime,
                            &mut stats,
                            &mut cx,
                        )
                        .await?;
                    }
                }
            }
            Ok(stats)
        })
    }

    async fn download_file_with_progress<F, C>(
        &self,
        remote: &str,
        local: &Path,
        file_size: u64,
        mtime: Option<u32>,
        stats: &mut TransferStats,
        cx: &mut TransferContext<'_, F, C>,
    ) -> Result<(), String>
    where
        F: FnMut(&ProgressUpdate),
        C: FnMut(&str) -> ConflictChoice,
    {
        let fname = rpath::file_name(remote);

        // Check for conflict before opening/overwriting local file
        if local.exists() {
            match (cx.on_conflict)(&fname) {
                ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {}
                ConflictChoice::Skip | ConflictChoice::SkipAll => {
                    stats.files += 1;
                    stats.bytes += file_size;
                    (cx.on_progress)(&ProgressUpdate {
                        file_name: fname,
                        file_bytes_transferred: file_size,
                        file_total_bytes: file_size,
                        completed_files: stats.files as usize,
                        total_bytes_transferred: stats.bytes,
                    });
                    return Ok(());
                }
                ConflictChoice::Cancel => {
                    return Err("Transfer cancelled by user".to_string());
                }
            }
        }

        let mut rf = self
            .sftp
            .open(remote)
            .await
            .map_err(|e| err("Open", remote, e))?;
        if let Some(parent) = local.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut lf = std::fs::File::create(local)
            .map_err(|e| err("Write", &local.display().to_string(), e))?;
        let mut buf = vec![0u8; CHUNK];
        let mut file_transferred = 0u64;
        let mut last_update = std::time::Instant::now();

        (cx.on_progress)(&ProgressUpdate {
            file_name: fname.clone(),
            file_bytes_transferred: 0,
            file_total_bytes: file_size,
            completed_files: stats.files as usize,
            total_bytes_transferred: stats.bytes,
        });

        loop {
            if cx.cancel.load(Ordering::Relaxed) {
                drop(lf);
                let _ = std::fs::remove_file(local);
                return Err("Transfer cancelled by user".to_string());
            }
            let n = rf
                .read(&mut buf)
                .await
                .map_err(|e| err("Download", remote, e))?;
            if n == 0 {
                break;
            }
            if let Err(e) = lf.write_all(&buf[..n]) {
                drop(lf);
                let _ = std::fs::remove_file(local);
                return Err(err("Write", &local.display().to_string(), e));
            }
            stats.bytes += n as u64;
            file_transferred += n as u64;

            let now = std::time::Instant::now();
            if now.duration_since(last_update) >= Duration::from_millis(50) {
                last_update = now;
                (cx.on_progress)(&ProgressUpdate {
                    file_name: fname.clone(),
                    file_bytes_transferred: file_transferred,
                    file_total_bytes: file_size,
                    completed_files: stats.files as usize,
                    total_bytes_transferred: stats.bytes,
                });
            }
        }
        let _ = rf.shutdown().await;
        if let Some(secs) = mtime {
            let _ = lf.set_modified(UNIX_EPOCH + Duration::from_secs(u64::from(secs)));
        }
        stats.files += 1;

        (cx.on_progress)(&ProgressUpdate {
            file_name: fname,
            file_bytes_transferred: file_transferred,
            file_total_bytes: file_size,
            completed_files: stats.files as usize,
            total_bytes_transferred: stats.bytes,
        });

        Ok(())
    }

    /// Uploads `local` (file or directory tree) to exactly `remote_dest`.
    pub fn upload(&self, local: &Path, remote_dest: &str) -> Result<TransferStats, String> {
        let cancel = AtomicBool::new(false);
        self.upload_with_progress(
            local,
            remote_dest,
            &cancel,
            |_| {},
            |_| ConflictChoice::Overwrite,
        )
    }

    /// Uploads `local` to `remote_dest` with cancellation, conflict resolution, and progress reporting.
    pub fn upload_with_progress<F, C>(
        &self,
        local: &Path,
        remote_dest: &str,
        cancel: &AtomicBool,
        mut on_progress: F,
        mut on_conflict: C,
    ) -> Result<TransferStats, String>
    where
        F: FnMut(&ProgressUpdate),
        C: FnMut(&str) -> ConflictChoice,
    {
        runtime().block_on(async {
            let mut stats = TransferStats::default();
            let mut cx = TransferContext {
                cancel,
                on_progress: &mut on_progress,
                on_conflict: &mut on_conflict,
            };
            let meta = std::fs::metadata(local)
                .map_err(|e| err("Stat", &local.display().to_string(), e))?;
            if !meta.is_dir() {
                self.upload_file_with_progress(local, remote_dest, meta.len(), &mut stats, &mut cx)
                    .await?;
                return Ok(stats);
            }
            let mut stack = vec![(local.to_path_buf(), rpath::normalize(remote_dest))];
            while let Some((ldir, rdir)) = stack.pop() {
                if cx.cancel.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".to_string());
                }
                if !self.sftp.try_exists(rdir.clone()).await.unwrap_or(false) {
                    self.sftp
                        .create_dir(rdir.clone())
                        .await
                        .map_err(|e| err("Create folder", &rdir, e))?;
                }
                stats.dirs += 1;
                let rd = std::fs::read_dir(&ldir)
                    .map_err(|e| err("List", &ldir.display().to_string(), e))?;
                for de in rd.flatten() {
                    if cx.cancel.load(Ordering::Relaxed) {
                        return Err("Transfer cancelled by user".to_string());
                    }
                    let name = de.file_name().to_string_lossy().into_owned();
                    let lchild = de.path();
                    let rchild = rpath::join(&rdir, &name);
                    if lchild.is_dir() {
                        stack.push((lchild, rchild));
                    } else {
                        let fsize = de.metadata().map(|m| m.len()).unwrap_or(0);
                        self.upload_file_with_progress(
                            &lchild, &rchild, fsize, &mut stats, &mut cx,
                        )
                        .await?;
                    }
                }
            }
            Ok(stats)
        })
    }

    async fn upload_file_with_progress<F, C>(
        &self,
        local: &Path,
        remote: &str,
        file_size: u64,
        stats: &mut TransferStats,
        cx: &mut TransferContext<'_, F, C>,
    ) -> Result<(), String>
    where
        F: FnMut(&ProgressUpdate),
        C: FnMut(&str) -> ConflictChoice,
    {
        let fname = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| remote.to_string());

        // Check for conflict before opening/truncating remote file
        if self
            .sftp
            .try_exists(remote.to_string())
            .await
            .unwrap_or(false)
        {
            match (cx.on_conflict)(&fname) {
                ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {}
                ConflictChoice::Skip | ConflictChoice::SkipAll => {
                    stats.files += 1;
                    stats.bytes += file_size;
                    (cx.on_progress)(&ProgressUpdate {
                        file_name: fname,
                        file_bytes_transferred: file_size,
                        file_total_bytes: file_size,
                        completed_files: stats.files as usize,
                        total_bytes_transferred: stats.bytes,
                    });
                    return Ok(());
                }
                ConflictChoice::Cancel => {
                    return Err("Transfer cancelled by user".to_string());
                }
            }
        }

        let mut lf =
            std::fs::File::open(local).map_err(|e| err("Read", &local.display().to_string(), e))?;
        let mut rf = self
            .sftp
            .open_with_flags(
                remote,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .map_err(|e| err("Create", remote, e))?;
        let mut buf = vec![0u8; CHUNK];
        let mut file_transferred = 0u64;
        let mut last_update = std::time::Instant::now();

        (cx.on_progress)(&ProgressUpdate {
            file_name: fname.clone(),
            file_bytes_transferred: 0,
            file_total_bytes: file_size,
            completed_files: stats.files as usize,
            total_bytes_transferred: stats.bytes,
        });

        loop {
            if cx.cancel.load(Ordering::Relaxed) {
                let _ = rf.shutdown().await;
                let _ = self.sftp.remove_file(remote.to_string()).await;
                return Err("Transfer cancelled by user".to_string());
            }
            let n = lf
                .read(&mut buf)
                .map_err(|e| err("Read", &local.display().to_string(), e))?;
            if n == 0 {
                break;
            }
            if let Err(e) = rf.write_all(&buf[..n]).await {
                let _ = rf.shutdown().await;
                let _ = self.sftp.remove_file(remote.to_string()).await;
                return Err(err("Upload", remote, e));
            }
            stats.bytes += n as u64;
            file_transferred += n as u64;

            let now = std::time::Instant::now();
            if now.duration_since(last_update) >= Duration::from_millis(50) {
                last_update = now;
                (cx.on_progress)(&ProgressUpdate {
                    file_name: fname.clone(),
                    file_bytes_transferred: file_transferred,
                    file_total_bytes: file_size,
                    completed_files: stats.files as usize,
                    total_bytes_transferred: stats.bytes,
                });
            }
        }
        rf.shutdown().await.map_err(|e| err("Upload", remote, e))?;
        // Preserve the modification time (best effort, like WinSCP's default).
        if let Ok(modified) = std::fs::metadata(local).and_then(|m| m.modified()) {
            if let Ok(d) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                let secs = d.as_secs().min(u64::from(u32::MAX)) as u32;
                let attrs = FileAttributes {
                    atime: Some(secs),
                    mtime: Some(secs),
                    ..FileAttributes::empty()
                };
                let _ = self.sftp.set_metadata(remote, attrs).await;
            }
        }
        stats.files += 1;

        (cx.on_progress)(&ProgressUpdate {
            file_name: fname,
            file_bytes_transferred: file_transferred,
            file_total_bytes: file_size,
            completed_files: stats.files as usize,
            total_bytes_transferred: stats.bytes,
        });

        Ok(())
    }

    /// Recursively counts files and total bytes of a remote file or directory.
    pub fn calc_remote_size(
        &self,
        path: &str,
        cancel: &AtomicBool,
    ) -> Result<(usize, u64), String> {
        runtime().block_on(async {
            let meta = self
                .sftp
                .metadata(path)
                .await
                .map_err(|e| err("Stat", path, e))?;
            if !meta.is_dir() {
                return Ok((1, meta.size.unwrap_or(0)));
            }
            let mut files = 0;
            let mut bytes = 0;
            let mut stack = vec![rpath::normalize(path)];
            while let Some(dir) = stack.pop() {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Transfer cancelled by user".to_string());
                }
                let rd = self
                    .sftp
                    .read_dir(dir.clone())
                    .await
                    .map_err(|e| err("List", &dir, e))?;
                for de in rd {
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Transfer cancelled by user".to_string());
                    }
                    let name = de.file_name();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let child = rpath::join(&dir, &name);
                    let mut m = de.metadata();
                    if m.file_type().is_symlink() {
                        match self.sftp.metadata(child.clone()).await {
                            Ok(t) => m = t,
                            Err(_) => continue,
                        }
                    }
                    if m.is_dir() {
                        stack.push(child);
                    } else {
                        files += 1;
                        bytes += m.size.unwrap_or(0);
                    }
                }
            }
            Ok((files, bytes))
        })
    }

    /// Size in bytes of a remote file (follows symlinks).
    pub fn file_size(&self, path: &str) -> Result<u64, String> {
        runtime()
            .block_on(self.sftp.metadata(path))
            .map(|m| m.size.unwrap_or(0))
            .map_err(|e| err("Stat", path, e))
    }

    pub fn disconnect(self) {
        let RemoteSession { handle, sftp, .. } = self;
        runtime().block_on(async move {
            let _ = tokio::time::timeout(Duration::from_secs(3), async {
                let _ = sftp.close().await;
                let _ = handle
                    .disconnect(russh::Disconnect::ByApplication, "", "en")
                    .await;
            })
            .await;
        });
    }
}

/// Recursively counts files and total bytes of a local file or directory.
pub fn calc_local_size(path: &Path) -> (usize, u64) {
    if !path.exists() {
        return (0, 0);
    }
    if !path.is_dir() {
        return (1, std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));
    }
    let mut files = 0;
    let mut bytes = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for de in rd.flatten() {
                if let Ok(ft) = de.file_type() {
                    if ft.is_dir() {
                        stack.push(de.path());
                    } else {
                        files += 1;
                        bytes += de.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
    }
    (files, bytes)
}

/// Replaces characters that are invalid in Windows file names.
pub fn sanitize_windows_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
        s.push('_');
        if s.chars().all(|c| c == '_') {
            break;
        }
    }
    if s.is_empty() {
        "_".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_windows_name_replaces_reserved_chars() {
        assert_eq!(sanitize_windows_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_windows_name("trail."), "trail_");
        assert_eq!(sanitize_windows_name("ok.txt"), "ok.txt");
    }

    #[test]
    fn calc_local_size_counts_nested_folder_files_and_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let sub1 = tmp.path().join("sub1");
        let sub2 = sub1.join("sub2");
        std::fs::create_dir_all(&sub2).unwrap();

        std::fs::write(tmp.path().join("f1.txt"), b"12345").unwrap(); // 5 bytes
        std::fs::write(sub1.join("f2.txt"), b"1234567890").unwrap(); // 10 bytes
        std::fs::write(sub2.join("f3.bin"), b"abc").unwrap(); // 3 bytes

        let (files, bytes) = calc_local_size(tmp.path());
        assert_eq!(files, 3);
        assert_eq!(bytes, 18);
    }
}
