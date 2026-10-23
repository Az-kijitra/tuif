//! Shared helpers for the `archive/` tests (test-only; `#[cfg(test)]` in
//! `archive/mod.rs`). `archive/` must not `use` `fs_ops`, so the link / tree
//! helpers of `fs_ops/test_support.rs` are re-implemented here. No production
//! logic. Uses `tempfile` (SPEC-0400-0001).

#![allow(dead_code)]

use super::error::{ArchiveError, ArchiveErrorCode};
use super::extract::{extract, ExtractHooks, ExtractOutcome, ExtractResult, Resolution};
use super::extract_plan::ExtractEnv;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Writes `contents` to `dir/relative` (creating parent directories).
pub fn write_file(dir: &Path, relative: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&path, contents).expect("write test file");
    path
}

pub fn read_bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

pub fn read_string(path: &Path) -> String {
    String::from_utf8(read_bytes(path)).expect("utf-8 file")
}

/// Whole tree below `dir` as `relative '/' path -> Some(bytes)` (file) or
/// `None` (directory). Links are followed only by `metadata`, tests do not
/// place links in trees they compare.
pub fn tree(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if entry.file_type().expect("file type").is_dir() {
                out.insert(rel, None);
                walk(base, &path, out);
            } else {
                out.insert(rel, Some(fs::read(&path).expect("read file")));
            }
        }
    }
    let mut out = BTreeMap::new();
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out
}

/// Only the relative names of `tree(dir)`.
pub fn tree_names(dir: &Path) -> Vec<String> {
    tree(dir).into_keys().collect()
}

/// No `.rfz-*.tmp` temp file anywhere below `dir` (R-328-1/2, R-348).
pub fn assert_no_temp_leftovers(dir: &Path) {
    let leftovers: Vec<String> = tree_names(dir)
        .into_iter()
        .filter(|n| {
            n.rsplit('/')
                .next()
                .map(|f| f.starts_with(".rfz-") || f.ends_with(".tmp"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}

/// Creates a directory link `link` -> `target`: an NTFS junction
/// (`mklink /J`, no elevation) on Windows, a symlink elsewhere.
pub fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

/// Removes a junction / directory link itself.
pub fn remove_dir_link(link: &Path) {
    #[cfg(windows)]
    {
        let _ = fs::remove_dir(link);
    }
    #[cfg(unix)]
    {
        let _ = fs::remove_file(link);
    }
}

/// A junction whose target directory no longer exists (a link with no target;
/// R-367 / R-372-2). Needs no elevation on Windows. Returns `false` if it
/// could not be made.
pub fn make_dangling_dir_link(link: &Path, scratch_target: &Path) -> bool {
    if fs::create_dir_all(scratch_target).is_err() {
        return false;
    }
    if !make_dir_link(link, scratch_target) {
        return false;
    }
    fs::remove_dir(scratch_target).is_ok()
}

/// A FILE symlink (needs the symlink privilege on Windows). `false` = not
/// possible here: the caller reports a skip.
pub fn make_file_symlink(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

pub fn skip(reason: &str) {
    eprintln!("SKIP: {reason}");
}

/// Sets the modification time of a file OR directory.
pub fn set_mtime(path: &Path, time: SystemTime) -> io::Result<()> {
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_BACKUP_SEMANTICS lets us open directories.
        fs::OpenOptions::new()
            .write(true)
            .custom_flags(0x0200_0000)
            .open(path)?
    };
    #[cfg(not(windows))]
    let file = fs::File::open(path)?;
    file.set_modified(time)
}

pub fn mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or_else(|e| panic!("mtime {}: {e}", path.display()))
}

/// `SystemTime` from a Unix timestamp.
pub fn unix_time(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs)
}

/// Absolute difference in whole seconds.
pub fn secs_apart(a: SystemTime, b: SystemTime) -> u64 {
    match a.duration_since(b) {
        Ok(d) => d.as_secs(),
        Err(e) => e.duration().as_secs(),
    }
}

fn plenty(_p: &Path) -> io::Result<u64> {
    Ok(u64::MAX)
}

/// An environment with unlimited free space.
pub fn env_plenty() -> ExtractEnv<'static> {
    ExtractEnv {
        free_space: &plenty,
    }
}

pub fn no_resolutions() -> HashMap<String, Resolution> {
    HashMap::new()
}

/// Extracts with unlimited space, no resolutions and no hooks.
pub fn extract_simple(zip: &Path, dest: &Path, base: &Path) -> Result<ExtractResult, ArchiveError> {
    extract(
        zip,
        dest,
        base,
        &env_plenty(),
        &no_resolutions(),
        &ExtractHooks::none(),
    )
}

/// The error code of a run regardless of whether the implementation reports it
/// as `Err` (rejected before writing) or as `Ok` with a non-completed outcome
/// (stopped after writing started). `None` = the run completed.
pub fn code_of(result: &Result<ExtractResult, ArchiveError>) -> Option<ArchiveErrorCode> {
    match result {
        Err(e) => Some(e.code),
        Ok(r) if r.outcome == ExtractOutcome::Completed => None,
        Ok(r) => Some(
            r.error
                .as_ref()
                .map(|e| e.code)
                .unwrap_or(ArchiveErrorCode::Unknown),
        ),
    }
}

/// UTF-16 length helper for building long-path fixtures.
pub fn repeat_char(c: char, n: usize) -> String {
    std::iter::repeat(c).take(n).collect()
}
