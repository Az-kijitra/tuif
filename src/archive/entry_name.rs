//! `archive/entry_name.rs` (SPEC-0300-0014 R-347; SPEC-0200-0001 §3.4.9-c).
//!
//! CP932 experiment result (`cp932_experiment_tests.rs`, zip 7.2): for an
//! entry without the UTF-8 flag the crate's `name()` is a CP437 rendering
//! (mojibake, and a CP932 second byte 0x5C becomes a real backslash), but
//! `name_raw()` returns the exact bytes -> design BRANCH 1: this module
//! decodes the raw bytes itself (`encoding_rs`). Tests:
//! `archive/entry_name_tests.rs`.

use super::error::{ArchiveError, ArchiveErrorCode};

/// R-347 policy: bit 11 set -> the raw bytes are UTF-8 (invalid UTF-8 there is
/// `invalid_path`, R-363 #11 / AC-364-3); bit 11 clear -> UTF-8 when the bytes
/// are valid UTF-8, otherwise CP932 (`encoding_rs::SHIFT_JIS`, the Windows-31J
/// superset). Bytes that cannot be decoded as CP932 are `invalid_path` too.
/// The result is then validated by `path_rules`.
pub fn resolve_entry_name(raw: &[u8], utf8_flag: bool) -> Result<String, ArchiveError> {
    if let Ok(text) = std::str::from_utf8(raw) {
        return Ok(text.to_string());
    }
    if utf8_flag {
        return Err(ArchiveError::new(
            ArchiveErrorCode::InvalidPath,
            "entry name is flagged as UTF-8 but is not valid UTF-8",
        ));
    }
    let (decoded, _, had_errors) = encoding_rs::SHIFT_JIS.decode(raw);
    if had_errors {
        return Err(ArchiveError::new(
            ArchiveErrorCode::InvalidPath,
            "entry name is neither valid UTF-8 nor valid CP932",
        ));
    }
    Ok(decoded.into_owned())
}
