//! `archive/eocd.rs` (SPEC-0300-0014 R-362 #1/#4, R-365 L-1; SPEC-0200-0001
//! §3.4.13-6, MUST-70). Reads ONLY the tail of the file (EOCD and, when
//! present, the ZIP64 EOCD locator/record) BEFORE `ZipArchive::new`.
//!
//! The `zip` crate opens multi-disk archives without complaint (experiment
//! E8), so the multi-disk rejection lives here. Tests: `archive/eocd_tests.rs`.

use super::error::{classify_io_error, ArchiveError, ArchiveErrorCode};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// L-1 (R-365): maximum number of entries.
pub const MAX_ENTRIES: u64 = 100_000;

const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const ZIP64_LOCATOR_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
const ZIP64_RECORD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const EOCD_LEN: usize = 22;
const ZIP64_LOCATOR_LEN: u64 = 20;
const ZIP64_RECORD_MIN_LEN: usize = 56;
const MAX_COMMENT: u64 = 65_535;

/// What the EOCD declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EocdInfo {
    /// Total number of central-directory entries (ZIP64 record when the
    /// 16-bit field is the 0xFFFF sentinel).
    pub total_entries: u64,
}

fn corrupt(message: &str) -> ArchiveError {
    ArchiveError::new(ArchiveErrorCode::ZipCorrupt, message)
}

fn unsupported(message: &str) -> ArchiveError {
    ArchiveError::new(ArchiveErrorCode::ZipUnsupported, message)
}

fn io_error(err: std::io::Error) -> ArchiveError {
    ArchiveError::new(classify_io_error(&err), err.to_string())
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn le64(bytes: &[u8], at: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(raw)
}

/// Judges the tail of a ZIP stream:
/// * no EOCD found (empty / too short / garbage) -> `zip_corrupt`;
/// * multi-disk (this-disk number != 0, central-directory disk != 0, entries on
///   this disk != total, ZIP64 locator/record with disk fields != 0 or total
///   disks > 1) -> `zip_unsupported`;
/// * declared total entries > `MAX_ENTRIES` -> `zip_too_large`.
///
/// The EOCD may be followed by a comment (search backwards up to 64 KiB + 22).
pub fn check_eocd<R: Read + Seek>(reader: &mut R) -> Result<EocdInfo, ArchiveError> {
    let length = reader.seek(SeekFrom::End(0)).map_err(io_error)?;
    let window = length.min(MAX_COMMENT + EOCD_LEN as u64);
    let window_start = length - window;
    reader
        .seek(SeekFrom::Start(window_start))
        .map_err(io_error)?;
    let mut tail = vec![0u8; window as usize];
    reader.read_exact(&mut tail).map_err(io_error)?;

    if tail.len() < EOCD_LEN {
        return Err(corrupt("the file is too short to be a ZIP archive"));
    }
    let position = (0..=tail.len() - EOCD_LEN)
        .rev()
        .find(|&i| {
            tail[i..i + 4] == EOCD_SIGNATURE
                && i + EOCD_LEN + le16(&tail, i + 20) as usize <= tail.len()
        })
        .ok_or_else(|| corrupt("no end-of-central-directory record found"))?;

    let this_disk = le16(&tail, position + 4);
    let cd_disk = le16(&tail, position + 6);
    let entries_this_disk = le16(&tail, position + 8);
    let entries_total = le16(&tail, position + 10);
    if this_disk != 0 || cd_disk != 0 || entries_this_disk != entries_total {
        return Err(unsupported("multi-volume ZIP archives are not supported"));
    }

    let mut total = entries_total as u64;
    let eocd_file_offset = window_start + position as u64;
    if eocd_file_offset >= ZIP64_LOCATOR_LEN {
        let locator_offset = eocd_file_offset - ZIP64_LOCATOR_LEN;
        reader
            .seek(SeekFrom::Start(locator_offset))
            .map_err(io_error)?;
        let mut locator = [0u8; ZIP64_LOCATOR_LEN as usize];
        reader.read_exact(&mut locator).map_err(io_error)?;
        if locator[..4] == ZIP64_LOCATOR_SIGNATURE {
            let locator_disk = le32(&locator, 4);
            let record_offset = le64(&locator, 8);
            let total_disks = le32(&locator, 16);
            if locator_disk != 0 || total_disks > 1 {
                return Err(unsupported("multi-volume ZIP archives are not supported"));
            }
            if record_offset
                .checked_add(ZIP64_RECORD_MIN_LEN as u64)
                .is_none_or(|end| end > length)
            {
                return Err(corrupt(
                    "the ZIP64 end-of-central-directory record is out of range",
                ));
            }
            reader
                .seek(SeekFrom::Start(record_offset))
                .map_err(io_error)?;
            let mut record = [0u8; ZIP64_RECORD_MIN_LEN];
            reader.read_exact(&mut record).map_err(io_error)?;
            if record[..4] != ZIP64_RECORD_SIGNATURE {
                return Err(corrupt(
                    "the ZIP64 end-of-central-directory record is missing",
                ));
            }
            let record_this_disk = le32(&record, 16);
            let record_cd_disk = le32(&record, 20);
            let record_entries_this_disk = le64(&record, 24);
            let record_entries_total = le64(&record, 32);
            if record_this_disk != 0
                || record_cd_disk != 0
                || record_entries_this_disk != record_entries_total
            {
                return Err(unsupported("multi-volume ZIP archives are not supported"));
            }
            // The ZIP64 count is authoritative only where the 16-bit field is
            // the 0xFFFF sentinel; otherwise the two must agree.
            if entries_total == 0xFFFF {
                total = record_entries_total;
            } else if u64::from(entries_total) != record_entries_total {
                return Err(corrupt(
                    "the entry counts of the ZIP64 and the ordinary end records disagree",
                ));
            }
        }
    }

    if total > MAX_ENTRIES {
        return Err(ArchiveError::new(
            ArchiveErrorCode::ZipTooLarge,
            format!("the archive declares {total} entries (limit {MAX_ENTRIES})"),
        ));
    }
    Ok(EocdInfo {
        total_entries: total,
    })
}

/// Convenience: opens `path` and runs `check_eocd`; I/O errors are classified
/// (`not_found`, `permission_denied`, `io_error`).
pub fn inspect(path: &Path) -> Result<EocdInfo, ArchiveError> {
    let mut file = File::open(path).map_err(io_error)?;
    check_eocd(&mut file)
}
