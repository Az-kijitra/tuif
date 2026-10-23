//! `viewer/classify.rs` (SPEC-0300-0015 R-410~R-414; SPEC-0200-0001 §3.5.1,
//! §3.5.3, §3.5.4: `decide_kind` is the ONE implementation shared by the
//! filer's open judgment and the previous/next display-capability check,
//! R-410-2/R-493). Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! pub const IMAGE_EXTENSIONS: &[&str];    // R-411: png, jpg, jpeg, bmp (4)
//! pub const MARKDOWN_EXTENSIONS: &[&str]; // R-411: md, markdown (2)
//! pub const TEXT_EXTENSIONS: &[&str];     // R-411: the 15-entry list
//! pub const OS_OPEN_EXTENSIONS: &[&str];  // R-414: the 10-entry direct list
//!
//! pub fn extension_of(path: &Path) -> Option<String>; // R-410-1
//! pub fn content_is_text(sample: &[u8]) -> bool;       // R-412 steps 1~5
//! pub fn decide_kind(ext: Option<&str>, sample: &[u8]) -> ViewerKind; // R-410
//! ```
//!
//! `sample` is ignored by `decide_kind` unless step 5 (content sniffing) is
//! reached (R-410 順5); callers pass up to the first 65,536 bytes (R-412).
//!
//! Tests: `viewer/classify_tests.rs`.

use std::path::Path;

/// R-410: the four possible outcomes of the open/don't-open judgment.
/// `OsOpen` covers both R-410 順4 (the direct list, R-414) and 順5 producing
/// "binary" (R-412) — both route to the OS association app, never the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerKind {
    Image,
    Markdown,
    Text,
    OsOpen,
}

/// R-411: images (4).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp"];
/// R-411: markdown (2).
pub const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown"];
/// R-411: text (15).
pub const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "log", "json", "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "rs", "js", "ts", "py",
];
/// R-414: the OS-association direct list (10).
pub const OS_OPEN_EXTENSIONS: &[&str] = &[
    "gif", "webp", "svg", "svgz", "ico", "tif", "tiff", "avif", "heic", "heif",
];

/// R-410-1: the text after the last dot, lowercased. A leading-dot-only name
/// (`.gitignore`) and a name with no dot at all (`Makefile`) have no
/// extension.
pub fn extension_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let dot = name.rfind('.')?;
    if dot == 0 {
        return None;
    }
    let ext = &name[dot + 1..];
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_lowercase())
}

/// R-412: the five-step content sniff.
pub fn content_is_text(sample: &[u8]) -> bool {
    // 順1: empty is text.
    if sample.is_empty() {
        return true;
    }
    // 順2: a BOM (UTF-8/UTF-16LE/UTF-16BE) short-circuits to text.
    if sample.starts_with(&[0xEF, 0xBB, 0xBF])
        || sample.starts_with(&[0xFF, 0xFE])
        || sample.starts_with(&[0xFE, 0xFF])
    {
        return true;
    }
    // 順3: a NUL byte anywhere in the sample is binary.
    if sample.contains(&0x00) {
        return false;
    }
    // 順4: control-byte ratio > 10% is binary (R-412-1/R-412-2).
    let control_count = sample.iter().filter(|&&b| is_control_byte(b)).count();
    let ratio = control_count as f64 / sample.len() as f64;
    if ratio > 0.10 {
        return false;
    }
    // 順5: otherwise text.
    true
}

/// R-412-1: the control-byte set (TAB/LF/FF/CR/ESC excluded).
fn is_control_byte(b: u8) -> bool {
    matches!(b, 0x01..=0x08 | 0x0B | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F)
}

fn matches_any(ext: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(ext))
}

/// R-410: the combined decision, the ONE implementation shared by the filer's
/// open judgment and the previous/next display-capability check (R-410-2).
pub fn decide_kind(ext: Option<&str>, sample: &[u8]) -> ViewerKind {
    if let Some(ext) = ext {
        if matches_any(ext, IMAGE_EXTENSIONS) {
            return ViewerKind::Image;
        }
        if matches_any(ext, MARKDOWN_EXTENSIONS) {
            return ViewerKind::Markdown;
        }
        if matches_any(ext, TEXT_EXTENSIONS) {
            return ViewerKind::Text;
        }
        if matches_any(ext, OS_OPEN_EXTENSIONS) {
            return ViewerKind::OsOpen;
        }
    }
    if content_is_text(sample) {
        ViewerKind::Text
    } else {
        ViewerKind::OsOpen
    }
}
