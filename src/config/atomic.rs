//! Atomic file write (SPEC-0300-0013 R-241, SPEC-0200-0001 §3.2.2, SHOULD-65).
//! Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()>
//! ```
//! Writes `<path>.tmp` (calling `sync_all()` before rename), then renames over
//! `path`; retries the rename a few times (e.g. 3x/50ms) on transient Windows
//! sharing violations. No external crates.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

const RENAME_ATTEMPTS: u32 = 3;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(50);

/// Writes `contents` to `<path>.tmp`, fsyncs, then renames over `path`.
/// Rename is retried on transient sharing violations (SHOULD-65).
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let mut tmp_name: OsString = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = Path::new(&tmp_name);

    {
        let mut file = File::create(tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }

    let mut attempt = 1;
    loop {
        match fs::rename(tmp, path) {
            Ok(()) => return Ok(()),
            Err(_) if attempt < RENAME_ATTEMPTS => {
                attempt += 1;
                thread::sleep(RENAME_RETRY_DELAY);
            }
            Err(e) => {
                let _ = fs::remove_file(tmp);
                return Err(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // R-241: creates the file with the given content, leaving no .tmp behind.
    #[test]
    fn writes_content_and_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("navigation.toml");

        atomic_write(&path, "hello = 1\n").expect("write");

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello = 1\n");
        assert!(!dir.path().join("navigation.toml.tmp").exists());
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "only the target file may remain");
    }

    // R-241: replaces an existing file wholesale (rename-over-existing).
    #[test]
    fn overwrites_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("navigation.toml");
        fs::write(&path, "old content that is longer").unwrap();

        atomic_write(&path, "new").expect("write");

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    // R-241 (AC-237): a stale .tmp from an earlier interrupted write does not
    // block or leak into the result.
    #[test]
    fn stale_tmp_file_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("navigation.toml");
        fs::write(dir.path().join("navigation.toml.tmp"), "garbage").unwrap();

        atomic_write(&path, "fresh").expect("write");

        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh");
        assert!(!dir.path().join("navigation.toml.tmp").exists());
    }

    // Errors are surfaced (not swallowed) when the parent directory is missing.
    #[test]
    fn missing_parent_directory_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no_such_dir").join("navigation.toml");
        assert!(atomic_write(&path, "x").is_err());
    }
}
