//! `archive/temp.rs` (SPEC-0300-0014 R-328, R-328-4, R-348, R-348-1;
//! SPEC-0200-0001 §3.4.13-6, SHOULD-87). Tests: `archive/temp_tests.rs`.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// `create_new` retry limit (§3.4.13-6): after this many name collisions the
/// creation fails with an `io::Error`.
pub const MAX_CREATE_NEW_ATTEMPTS: u32 = 100;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// `.rfz-<pid>-<counter>.tmp` — fixed shape, independent of any final name
/// (R-328-4 / R-348-1).
pub fn temp_file_name(pid: u32, counter: u64) -> String {
    format!(".rfz-{pid}-{counter}.tmp")
}

/// A temporary file that is deleted on drop (RAII, SHOULD-87) unless
/// `into_path` is called.
pub struct TempFile {
    path: PathBuf,
    file: Option<File>,
}

impl TempFile {
    /// Creates a fresh temp file in `dir` (`create_new`), names from
    /// `temp_file_name(process id, process-wide counter)`.
    pub fn create_in(dir: &Path) -> io::Result<TempFile> {
        let pid = std::process::id();
        let mut next = || temp_file_name(pid, COUNTER.fetch_add(1, Ordering::Relaxed));
        TempFile::create_in_with(dir, &mut next)
    }

    /// Same, with an injectable name generator (tests). Calls `next_name` once
    /// per attempt; on `AlreadyExists` it retries, at most
    /// `MAX_CREATE_NEW_ATTEMPTS` attempts in total, then returns an error.
    /// An existing file is never truncated or reused.
    pub fn create_in_with(
        dir: &Path,
        next_name: &mut dyn FnMut() -> String,
    ) -> io::Result<TempFile> {
        for _ in 0..MAX_CREATE_NEW_ATTEMPTS {
            let path = dir.join(next_name());
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(TempFile {
                        path,
                        file: Some(file),
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other(
            "could not create a unique temporary file (retry limit reached)",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The open handle for writing.
    pub fn file(&mut self) -> &mut File {
        self.file.as_mut().expect("temp file handle")
    }

    /// Closes the handle but keeps the delete-on-drop armed (the caller is
    /// about to rename the file; a failed rename still cleans up).
    pub fn close_handle(&mut self) {
        self.file = None;
    }

    /// Closes the handle, disarms the delete-on-drop and returns the path
    /// (the caller then renames it to its final name).
    pub fn into_path(mut self) -> PathBuf {
        self.file = None;
        std::mem::take(&mut self.path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        // After `into_path` the path is empty: nothing to delete.
        if self.path.as_os_str().is_empty() {
            return;
        }
        self.file = None; // close the handle first (required on Windows)
        let _ = fs::remove_file(&self.path);
    }
}
