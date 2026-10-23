//! TDD (unit-test-writer, 2026-09-27): error codes, DTO shape and the io / zip
//! error classifiers.
//!
//! Specs: SPEC-0300-0014 R-394, R-395 (code table), R-395-1 (entryPath),
//! R-395-2 (disk full = `io_error`), R-395-3 (`permission_denied`), R-395-4,
//! AC-381; SPEC-0200-0001 §3.4.2 ("classify by variant / ErrorKind, never by
//! message text").

use super::error::*;
use std::io;

// R-395: the wire strings are exactly the specified codes.
#[test]
fn r395_wire_strings_match_the_specified_code_table() {
    let expected = [
        (ArchiveErrorCode::NoItems, "no_items"),
        (ArchiveErrorCode::NotFound, "not_found"),
        (ArchiveErrorCode::InvalidPath, "invalid_path"),
        (ArchiveErrorCode::InvalidName, "invalid_name"),
        (ArchiveErrorCode::AlreadyExists, "already_exists"),
        (
            ArchiveErrorCode::DestinationInsideSource,
            "destination_inside_source",
        ),
        (
            ArchiveErrorCode::DestinationNotFound,
            "destination_not_found",
        ),
        (ArchiveErrorCode::DestinationNotDir, "destination_not_dir"),
        (ArchiveErrorCode::ZipCorrupt, "zip_corrupt"),
        (ArchiveErrorCode::ZipUnsupported, "zip_unsupported"),
        (ArchiveErrorCode::ZipEncrypted, "zip_encrypted"),
        (ArchiveErrorCode::ZipEntryRejected, "zip_entry_rejected"),
        (ArchiveErrorCode::ZipTooLarge, "zip_too_large"),
        (ArchiveErrorCode::InsufficientSpace, "insufficient_space"),
        (ArchiveErrorCode::Conflict, "conflict"),
        (ArchiveErrorCode::PermissionDenied, "permission_denied"),
        (ArchiveErrorCode::IoError, "io_error"),
        (ArchiveErrorCode::Unknown, "unknown"),
        (ArchiveErrorCode::Canceled, "canceled"),
    ];
    assert_eq!(expected.len(), ArchiveErrorCode::ALL.len());
    for (code, wire) in expected {
        assert_eq!(code.as_str(), wire);
    }
}

// R-395-1 / R-395-4: the DTO keeps code, message (library detail intact) and
// the entry path.
#[test]
fn r395_1_dto_carries_code_message_and_entry_path() {
    let err = ArchiveError::new(
        ArchiveErrorCode::ZipEntryRejected,
        "entry `../x` escapes the destination",
    )
    .with_entry("../x");
    let dto = ArchiveErrorDto::from(err);
    assert_eq!(dto.code, "zip_entry_rejected");
    assert_eq!(dto.message, "entry `../x` escapes the destination");
    assert_eq!(dto.entry_path.as_deref(), Some("../x"));
}

#[test]
fn r395_1_dto_serializes_with_camel_case_keys() {
    let dto = ArchiveErrorDto::from(
        ArchiveError::new(ArchiveErrorCode::ZipCorrupt, "bad").with_entry("a.txt"),
    );
    let text = toml::to_string(&dto).expect("serialize");
    assert!(text.contains("entryPath = \"a.txt\""), "{text}");
    assert!(text.contains("code = \"zip_corrupt\""), "{text}");
    assert!(!text.contains("entry_path"), "{text}");
}

// R-395-3: permission errors are classified by ErrorKind.
#[test]
fn r395_3_permission_denied_kind_maps_to_permission_denied() {
    let e = io::Error::from(io::ErrorKind::PermissionDenied);
    assert_eq!(classify_io_error(&e), ArchiveErrorCode::PermissionDenied);
}

// R-395-2 / AC-380-1: a full disk is `io_error`, not a dedicated code.
#[test]
fn r395_2_storage_full_maps_to_io_error() {
    let e = io::Error::from(io::ErrorKind::StorageFull);
    assert_eq!(classify_io_error(&e), ArchiveErrorCode::IoError);
}

// Windows raw codes ERROR_DISK_FULL (112) / ERROR_HANDLE_DISK_FULL (39).
#[cfg(windows)]
#[test]
fn r395_2_windows_disk_full_raw_codes_map_to_io_error() {
    assert_eq!(
        classify_io_error(&io::Error::from_raw_os_error(112)),
        ArchiveErrorCode::IoError
    );
    assert_eq!(
        classify_io_error(&io::Error::from_raw_os_error(39)),
        ArchiveErrorCode::IoError
    );
}

#[test]
fn classify_io_error_by_kind_table() {
    let cases = [
        (io::ErrorKind::NotFound, ArchiveErrorCode::NotFound),
        (io::ErrorKind::AlreadyExists, ArchiveErrorCode::Conflict),
        // Source read / destination write errors while creating are not archive
        // corruption; the read side of extraction has its own mapping.
        (io::ErrorKind::InvalidData, ArchiveErrorCode::IoError),
        (io::ErrorKind::UnexpectedEof, ArchiveErrorCode::IoError),
        (io::ErrorKind::Other, ArchiveErrorCode::IoError),
        (io::ErrorKind::TimedOut, ArchiveErrorCode::IoError),
    ];
    for (kind, code) in cases {
        assert_eq!(classify_io_error(&io::Error::from(kind)), code, "{kind:?}");
    }
}

// SPEC-0200-0001 §3.4.2: NEVER classify by message text.
#[test]
fn classification_ignores_message_text() {
    let e = io::Error::other("Access is denied. (permission denied) file already exists");
    assert_eq!(classify_io_error(&e), ArchiveErrorCode::IoError);
}

// AC-381: zip crate errors are classified by variant, never `unknown`.
#[test]
fn ac381_zip_error_variants_map_to_zip_codes() {
    use zip::result::ZipError;
    assert_eq!(
        classify_zip_error(&ZipError::InvalidArchive(std::borrow::Cow::Borrowed("bad"))),
        ArchiveErrorCode::ZipCorrupt
    );
    assert_eq!(
        classify_zip_error(&ZipError::UnsupportedArchive("x")),
        ArchiveErrorCode::ZipUnsupported
    );
    assert_eq!(
        classify_zip_error(&ZipError::FileNotFound),
        ArchiveErrorCode::ZipCorrupt
    );
    assert_eq!(
        classify_zip_error(&ZipError::Io(io::Error::from(
            io::ErrorKind::PermissionDenied
        ))),
        ArchiveErrorCode::PermissionDenied
    );
    assert_eq!(
        classify_zip_error(&ZipError::Io(io::Error::from(io::ErrorKind::InvalidData))),
        ArchiveErrorCode::ZipCorrupt
    );
}

// The zip crate reports a truncated / broken container as an `Io` error of
// these kinds; that mapping stays with `classify_zip_error`.
#[test]
fn zip_io_errors_of_data_kinds_stay_zip_corrupt() {
    use zip::result::ZipError;
    for kind in [io::ErrorKind::InvalidData, io::ErrorKind::UnexpectedEof] {
        assert_eq!(
            classify_zip_error(&ZipError::Io(io::Error::from(kind))),
            ArchiveErrorCode::ZipCorrupt,
            "{kind:?}"
        );
    }
}
