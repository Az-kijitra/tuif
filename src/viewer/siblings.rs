//! `viewer/siblings.rs` (SPEC-0300-0015 R-491, R-491-1~3, R-492;
//! SPEC-0200-0001 §3.5.1/§3.5.4: `fs::read_dir` lives ONLY here — the viewer
//! never calls `fs::query`'s `fs_list_dir` (R-442-1), and this is not exposed
//! as a Tauri command of its own).
//!
//! Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! // R-491: ordinal (name, case-insensitive) comparison with R-491-3's
//! // tie-break (same names differing only by case: compare the ORIGINAL
//! // strings ordinally as the tie-break, UTF-16 code-unit order).
//! pub fn compare_names(a: &str, b: &str) -> std::cmp::Ordering;
//!
//! // R-491/R-491-1/R-491-2: every FILE (not directory) directly under `dir`,
//! // hidden files included, sorted with `compare_names`. Symlinks/junctions
//! // are included as entries (R-495 decides readability later, at the
//! // session-allow-set layer, not here).
//! pub fn build_sibling_list(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>>;
//! ```
//!
//! Tests: `viewer/siblings_tests.rs`.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// R-491/R-491-3: case-insensitive ordinal comparison, tied names broken by
/// the original (UTF-16 code-unit order, approximated here by Rust's default
/// `str` ordinal `Ord`, which agrees with UTF-16 code-unit order for the BMP)
/// ordinal comparison of the original strings.
pub fn compare_names(a: &str, b: &str) -> Ordering {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    a_lower.cmp(&b_lower).then_with(|| a.cmp(b))
}

/// R-491/R-491-1/R-491-2: every FILE (not directory) directly under `dir`,
/// hidden files included, sorted with `compare_names`.
pub fn build_sibling_list(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            continue;
        }
        files.push(entry.path());
    }
    files.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        compare_names(a_name, b_name)
    });
    Ok(files)
}
