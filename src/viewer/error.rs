//! `viewer/error.rs` (SPEC-0300-0015 R-440, R-447; SPEC-0300-0011 第6版 R-85
//! — the viewer error codes must be string-compatible with that table,
//! ISS-0128). Mirrors `archive/error.rs`'s shape (SPEC-0200-0001 §3.5.9: the
//! Rust error enum is NOT shared across features; only the wire strings are
//! kept consistent).
//!
//! Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! impl ViewerErrorCode {
//!     pub fn as_str(self) -> &'static str; // exact R-440 table strings
//! }
//! impl From<ViewerError> for ViewerErrorDto { ... }
//! pub fn classify_io_error(err: &std::io::Error) -> ViewerErrorCode; // by ErrorKind, R-440
//! ```
//!
//! Tests: `viewer/error_tests.rs`.

use serde::Serialize;

/// R-440 / R-447: the machine-readable viewer error codes. `NotAFile` and
/// `ViewerPathNotAllowed` and `ViewerImageTooLarge` are new (SPEC-0300-0011
/// 第6版 R-85, ISS-0128); the rest reuse existing SPEC-0300-0011 R-85 codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewerErrorCode {
    NotFound,
    NotAFile,
    PermissionDenied,
    ViewerPathNotAllowed,
    ViewerImageTooLarge,
    IoError,
    Unknown,
    /// R-89-2 / R-440: a path argument that is not an absolute path (SPEC-0300-0011 R-85-4).
    InvalidPath,
}

impl ViewerErrorCode {
    /// Every code, for exhaustive contract tests (mirrors `ArchiveErrorCode::ALL`).
    pub const ALL: [ViewerErrorCode; 8] = [
        ViewerErrorCode::NotFound,
        ViewerErrorCode::NotAFile,
        ViewerErrorCode::PermissionDenied,
        ViewerErrorCode::ViewerPathNotAllowed,
        ViewerErrorCode::ViewerImageTooLarge,
        ViewerErrorCode::IoError,
        ViewerErrorCode::Unknown,
        ViewerErrorCode::InvalidPath,
    ];
}

/// R-447: code + a user-facing message (translation happens on the frontend,
/// `features/viewer/error-format.ts`; this struct only carries the code+raw
/// message across IPC).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerError {
    pub code: ViewerErrorCode,
    pub message: String,
}

/// IPC form (`{ code, message }`, camelCase — no `entryPath`, unlike archive's
/// DTO: the viewer never operates on archive-internal paths).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerErrorDto {
    pub code: String,
    pub message: String,
}

impl ViewerErrorCode {
    /// R-440/R-447: the exact wire string for this code.
    pub fn as_str(self) -> &'static str {
        match self {
            ViewerErrorCode::NotFound => "not_found",
            ViewerErrorCode::NotAFile => "not_a_file",
            ViewerErrorCode::PermissionDenied => "permission_denied",
            ViewerErrorCode::ViewerPathNotAllowed => "viewer_path_not_allowed",
            ViewerErrorCode::ViewerImageTooLarge => "viewer_image_too_large",
            ViewerErrorCode::IoError => "io_error",
            ViewerErrorCode::Unknown => "unknown",
            ViewerErrorCode::InvalidPath => "invalid_path",
        }
    }
}

impl From<ViewerError> for ViewerErrorDto {
    fn from(err: ViewerError) -> Self {
        ViewerErrorDto { code: err.code.as_str().to_string(), message: err.message }
    }
}

/// R-440: classifies an `io::Error` by `ErrorKind` only (never by message text).
pub fn classify_io_error(err: &std::io::Error) -> ViewerErrorCode {
    match err.kind() {
        std::io::ErrorKind::NotFound => ViewerErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => ViewerErrorCode::PermissionDenied,
        _ => ViewerErrorCode::IoError,
    }
}
