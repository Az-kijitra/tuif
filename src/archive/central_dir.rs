//! `archive/central_dir.rs` (SPEC-0200-0001 §3.4.4, experiment findings in
//! `cp932_experiment_tests.rs`): a minimal own scan of the central directory.
//!
//! The `zip` crate hides three things the validation needs: (1) exactly
//! duplicated names are silently merged into one entry (F8), (2) an AES entry
//! (method 99) without general-purpose bit 0 looks like an ordinary Deflate
//! entry (E3), (3) the raw method / flag fields. Only name bytes, flags and the
//! method are read here; sizes, times and attributes come from the crate.

use super::error::{classify_io_error, ArchiveError, ArchiveErrorCode};
use std::io::{BufReader, Read, Seek, SeekFrom};

const CENTRAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const CENTRAL_HEADER_LEN: usize = 46;

/// One central-directory record (raw fields only).
#[derive(Debug, Clone)]
pub struct CdRecord {
    pub name_raw: Vec<u8>,
    /// General-purpose bit flags (bit 0 = encrypted, bit 11 = UTF-8 names).
    pub flags: u16,
    /// Raw compression method number (99 = AES marker).
    pub method: u16,
}

fn corrupt(message: &str) -> ArchiveError {
    ArchiveError::new(ArchiveErrorCode::ZipCorrupt, message)
}

/// Reads `count` records starting at `start`.
pub fn scan<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    count: usize,
) -> Result<Vec<CdRecord>, ArchiveError> {
    let io = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            corrupt("the central directory is truncated")
        } else {
            ArchiveError::new(classify_io_error(&e), e.to_string())
        }
    };
    reader.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut reader = BufReader::new(reader);
    let mut records = Vec::with_capacity(count.min(100_001));
    let mut header = [0u8; CENTRAL_HEADER_LEN];
    for _ in 0..count {
        reader.read_exact(&mut header).map_err(io)?;
        if header[..4] != CENTRAL_HEADER_SIGNATURE {
            return Err(corrupt("bad central directory record signature"));
        }
        let flags = u16::from_le_bytes([header[8], header[9]]);
        let method = u16::from_le_bytes([header[10], header[11]]);
        let name_len = u16::from_le_bytes([header[28], header[29]]) as usize;
        let extra_len = u16::from_le_bytes([header[30], header[31]]) as i64;
        let comment_len = u16::from_le_bytes([header[32], header[33]]) as i64;
        let mut name_raw = vec![0u8; name_len];
        reader.read_exact(&mut name_raw).map_err(io)?;
        reader.seek_relative(extra_len + comment_len).map_err(io)?;
        records.push(CdRecord {
            name_raw,
            flags,
            method,
        });
    }
    Ok(records)
}
