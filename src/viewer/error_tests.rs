//! TDD (unit-test-writer, 2026-09-27): the R-440 wire-string table, the
//! `ViewerError` -> `ViewerErrorDto` conversion, and the `io::Error` classifier.
//!
//! Specs: SPEC-0300-0015 R-440, R-447; SPEC-0300-0011 第6版 R-85 (viewer
//! codes: `not_a_file`, `viewer_path_not_allowed`, `viewer_image_too_large`,
//! ISS-0128); AC-450.

use super::error::*;
use std::io;

// R-440: the wire strings match the specified code table exactly.
#[test]
fn r440_wire_strings_match_the_specified_code_table() {
    let expected = [
        (ViewerErrorCode::NotFound, "not_found"),
        (ViewerErrorCode::NotAFile, "not_a_file"),
        (ViewerErrorCode::PermissionDenied, "permission_denied"),
        (
            ViewerErrorCode::ViewerPathNotAllowed,
            "viewer_path_not_allowed",
        ),
        (
            ViewerErrorCode::ViewerImageTooLarge,
            "viewer_image_too_large",
        ),
        (ViewerErrorCode::IoError, "io_error"),
        (ViewerErrorCode::Unknown, "unknown"),
        (ViewerErrorCode::InvalidPath, "invalid_path"),
    ];
    assert_eq!(expected.len(), ViewerErrorCode::ALL.len());
    for (code, wire) in expected {
        assert_eq!(code.as_str(), wire);
    }
}

// AC-450: `not_found` / `not_a_file` come from ErrorKind, never from message text.
#[test]
fn classify_io_error_not_found_is_not_found() {
    let e = io::Error::new(io::ErrorKind::NotFound, "x");
    assert_eq!(classify_io_error(&e), ViewerErrorCode::NotFound);
}

#[test]
fn classify_io_error_permission_denied_is_permission_denied() {
    let e = io::Error::new(io::ErrorKind::PermissionDenied, "x");
    assert_eq!(classify_io_error(&e), ViewerErrorCode::PermissionDenied);
}

#[test]
fn classify_io_error_everything_else_is_io_error() {
    let e = io::Error::new(io::ErrorKind::Other, "disk full, or whatever");
    assert_eq!(classify_io_error(&e), ViewerErrorCode::IoError);
}

// R-447: the DTO carries the wire string, not the Rust-only enum.
#[test]
fn dto_conversion_carries_the_wire_code_and_message() {
    let err = ViewerError {
        code: ViewerErrorCode::ViewerPathNotAllowed,
        message: "nope".into(),
    };
    let dto: ViewerErrorDto = err.into();
    assert_eq!(dto.code, "viewer_path_not_allowed");
    assert_eq!(dto.message, "nope");
}

// R-447: internal representation (`code=...`) never leaks as the message text.
#[test]
fn the_dto_message_never_looks_like_the_internal_debug_form() {
    let err = ViewerError {
        code: ViewerErrorCode::IoError,
        message: "disk error".into(),
    };
    let dto: ViewerErrorDto = err.into();
    assert!(!dto.message.starts_with("code="));
}

// ---- SPEC-0300-0015 第6版 R-440 / R-447 (unit-test-writer, 2026-10-07) -------------------------------
// `ViewerErrorCode::InvalidPath` (wire `"invalid_path"`, same string as SPEC-0300-0011 R-85 / archive) joins
// `ALL` (now 8 values). The test above (`r440_wire_strings_match_the_specified_code_table`) lists all 8 values.

#[test]
fn r440_invalid_path_is_the_eighth_code_and_all_wire_strings_are_distinct() {
    let wires: Vec<&str> = ViewerErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(ViewerErrorCode::ALL.len(), 8, "{wires:?}");
    assert!(wires.contains(&"invalid_path"), "{wires:?}");
    for existing in [
        "not_found",
        "not_a_file",
        "permission_denied",
        "viewer_path_not_allowed",
        "viewer_image_too_large",
        "io_error",
        "unknown",
    ] {
        assert!(wires.contains(&existing), "{existing} must stay: {wires:?}");
    }
    let mut sorted = wires.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 8, "wire strings are distinct: {wires:?}");
}

// `classify_io_error` is NOT changed: no `ErrorKind` becomes `invalid_path`.
#[test]
fn classify_io_error_never_returns_invalid_path() {
    for kind in [
        io::ErrorKind::InvalidInput,
        io::ErrorKind::InvalidData,
        io::ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::Other,
    ] {
        assert_ne!(
            classify_io_error(&io::Error::new(kind, "x")).as_str(),
            "invalid_path",
            "{kind:?}"
        );
    }
}

#[test]
fn the_invalid_path_code_reaches_the_dto_unchanged() {
    let code = ViewerErrorCode::ALL
        .iter()
        .copied()
        .find(|c| c.as_str() == "invalid_path")
        .expect("InvalidPath in ALL");
    let dto: ViewerErrorDto = ViewerError {
        code,
        message: "絶対パスではありません".into(),
    }
    .into();
    assert_eq!(dto.code, "invalid_path");
    assert_eq!(dto.message, "絶対パスではありません");
}
