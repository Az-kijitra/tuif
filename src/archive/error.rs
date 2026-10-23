//! `archive/error.rs` (SPEC-0300-0014 R-394 / R-395; SPEC-0200-0001 §3.4.2):
//! error codes, the error type, its IPC form and the io / zip classifiers.
//! Tests: `archive/error_tests.rs`.

use serde::Serialize;
use std::fmt;

/// Machine-readable error codes (R-395). Existing codes reuse SPEC-0300-0011
/// R-85; the `Zip*` / `InsufficientSpace` codes are new.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchiveErrorCode {
    NoItems,
    NotFound,
    InvalidPath,
    InvalidName,
    AlreadyExists,
    DestinationInsideSource,
    DestinationNotFound,
    DestinationNotDir,
    ZipCorrupt,
    ZipUnsupported,
    ZipEncrypted,
    ZipEntryRejected,
    ZipTooLarge,
    InsufficientSpace,
    Conflict,
    PermissionDenied,
    IoError,
    Unknown,
    Canceled,
}

impl ArchiveErrorCode {
    /// Every code, for exhaustive contract tests.
    pub const ALL: [ArchiveErrorCode; 19] = [
        ArchiveErrorCode::NoItems,
        ArchiveErrorCode::NotFound,
        ArchiveErrorCode::InvalidPath,
        ArchiveErrorCode::InvalidName,
        ArchiveErrorCode::AlreadyExists,
        ArchiveErrorCode::DestinationInsideSource,
        ArchiveErrorCode::DestinationNotFound,
        ArchiveErrorCode::DestinationNotDir,
        ArchiveErrorCode::ZipCorrupt,
        ArchiveErrorCode::ZipUnsupported,
        ArchiveErrorCode::ZipEncrypted,
        ArchiveErrorCode::ZipEntryRejected,
        ArchiveErrorCode::ZipTooLarge,
        ArchiveErrorCode::InsufficientSpace,
        ArchiveErrorCode::Conflict,
        ArchiveErrorCode::PermissionDenied,
        ArchiveErrorCode::IoError,
        ArchiveErrorCode::Unknown,
        ArchiveErrorCode::Canceled,
    ];

    /// The wire string (R-395 table).
    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveErrorCode::NoItems => "no_items",
            ArchiveErrorCode::NotFound => "not_found",
            ArchiveErrorCode::InvalidPath => "invalid_path",
            ArchiveErrorCode::InvalidName => "invalid_name",
            ArchiveErrorCode::AlreadyExists => "already_exists",
            ArchiveErrorCode::DestinationInsideSource => "destination_inside_source",
            ArchiveErrorCode::DestinationNotFound => "destination_not_found",
            ArchiveErrorCode::DestinationNotDir => "destination_not_dir",
            ArchiveErrorCode::ZipCorrupt => "zip_corrupt",
            ArchiveErrorCode::ZipUnsupported => "zip_unsupported",
            ArchiveErrorCode::ZipEncrypted => "zip_encrypted",
            ArchiveErrorCode::ZipEntryRejected => "zip_entry_rejected",
            ArchiveErrorCode::ZipTooLarge => "zip_too_large",
            ArchiveErrorCode::InsufficientSpace => "insufficient_space",
            ArchiveErrorCode::Conflict => "conflict",
            ArchiveErrorCode::PermissionDenied => "permission_denied",
            ArchiveErrorCode::IoError => "io_error",
            ArchiveErrorCode::Unknown => "unknown",
            ArchiveErrorCode::Canceled => "canceled",
        }
    }
}

/// R-395-1 / R-395-4: code + human message (+ the problematic archive entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveError {
    pub code: ArchiveErrorCode,
    pub message: String,
    /// Archive-internal path of the offending entry, when there is one.
    pub entry_path: Option<String>,
}

impl ArchiveError {
    pub fn new(code: ArchiveErrorCode, message: impl Into<String>) -> Self {
        ArchiveError {
            code,
            message: message.into(),
            entry_path: None,
        }
    }

    pub fn with_entry(mut self, entry_path: impl Into<String>) -> Self {
        self.entry_path = Some(entry_path.into());
        self
    }
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ArchiveError {}

/// IPC form of `ArchiveError` (`{ code, message, entryPath? }`, camelCase).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveErrorDto {
    pub code: String,
    pub message: String,
    pub entry_path: Option<String>,
}

impl From<ArchiveError> for ArchiveErrorDto {
    fn from(err: ArchiveError) -> Self {
        ArchiveErrorDto {
            code: err.code.as_str().to_string(),
            message: err.message,
            entry_path: err.entry_path,
        }
    }
}

/// `io::Error` -> code by `ErrorKind` (never by message text, §3.4.2):
/// `PermissionDenied` -> `permission_denied`, `NotFound` -> `not_found`,
/// `AlreadyExists` -> `conflict` (no-replace commit), everything else (disk full etc., R-395-2) -> `io_error`.
pub fn classify_io_error(err: &std::io::Error) -> ArchiveErrorCode {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::PermissionDenied => ArchiveErrorCode::PermissionDenied,
        ErrorKind::NotFound => ArchiveErrorCode::NotFound,
        ErrorKind::AlreadyExists => ArchiveErrorCode::Conflict,
        _ => ArchiveErrorCode::IoError,
    }
}

/// `zip::result::ZipError` -> code by variant (§3.4.2):
/// `InvalidArchive` -> `zip_corrupt`, `UnsupportedArchive` -> `zip_unsupported`,
/// `Io(e)` of kind `InvalidData` / `UnexpectedEof` (truncated or broken
/// container data) -> `zip_corrupt`, other `Io(e)` -> `classify_io_error(e)`, anything else -> `zip_corrupt`.
pub fn classify_zip_error(err: &zip::result::ZipError) -> ArchiveErrorCode {
    use zip::result::ZipError;
    match err {
        ZipError::InvalidArchive(_) => ArchiveErrorCode::ZipCorrupt,
        ZipError::UnsupportedArchive(_) => ArchiveErrorCode::ZipUnsupported,
        ZipError::Io(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof
            ) =>
        {
            ArchiveErrorCode::ZipCorrupt
        }
        ZipError::Io(e) => classify_io_error(e),
        _ => ArchiveErrorCode::ZipCorrupt,
    }
}
