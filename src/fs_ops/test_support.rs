//! Shared helpers for the Phase 2 wiring tests (SPEC-0300-0011 4th edition:
//! `detect_conflicts_tests`, `overwrite_merge_tests`, `data_safety_tests`).
//! Test-only (`#[cfg(test)]` in `fs_ops/mod.rs`); contains no production
//! logic. Uses `tempfile` per SPEC-0400-0001.

use std::fs;
use std::path::{Path, PathBuf};

pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Writes `contents` to `dir/relative` (creating parent directories).
pub fn write(dir: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = dir.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&path, contents).expect("write test file");
    path
}

pub fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Sorted entry names directly under `dir`.
pub fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// Creates a directory link `link` -> `target`: an NTFS junction
/// (`mklink /J`, no elevation needed) on Windows, a symlink elsewhere.
/// R-37-1 (junction / symlink aliases of the same entity).
pub fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
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

/// Removes a junction/symlink itself (never its target's contents).
pub fn remove_dir_link(link: &Path) {
    // `remove_dir` removes a junction / directory symlink without touching
    // the target on Windows; on unix a dir symlink needs `remove_file`.
    #[cfg(windows)]
    {
        let _ = fs::remove_dir(link);
    }
    #[cfg(unix)]
    {
        let _ = fs::remove_file(link);
    }
}

/// Upper-cases the last component of `path` (Windows is case-insensitive, so
/// this names the same entry with a different spelling; R-37-1, AC-78).
#[cfg(windows)]
pub fn with_last_component_uppercased(path: &Path) -> PathBuf {
    let name = path.file_name().expect("file name").to_string_lossy().to_uppercase();
    path.parent().expect("parent").join(name)
}
