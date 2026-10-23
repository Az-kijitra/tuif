//! Test-only ZIP builder (SPEC-0200-0001 §3.4.10 `zip_fixture.rs`): assembles ZIP
//! bytes by hand (local headers, central directory, EOCD, ZIP64) so that tests
//! can produce archives the `zip` crate's writer refuses to write: forged
//! sizes / CRCs, encryption flags, method 99, symlink entries, a central
//! directory name that differs from the local header name, unpaired surrogates,
//! CP932 names, multi-disk EOCDs, forged entry counts, ...
//!
//! Deflate entries are written as raw deflate STORED blocks (valid deflate, no
//! compression), so no compression library is needed here. Nothing in this
//! file is production code (`#[cfg(test)]` in `archive/mod.rs`).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub const FLAG_ENCRYPTED: u16 = 0x0001;
pub const FLAG_UTF8: u16 = 0x0800;

/// Method numbers used by the tests.
pub const METHOD_STORED: u16 = 0;
pub const METHOD_DEFLATE: u16 = 8;
pub const METHOD_BZIP2: u16 = 12;
pub const METHOD_LZMA: u16 = 14;
pub const METHOD_AES: u16 = 99;

/// `(host << 8) | version` for "made by".
pub const MADE_BY_DOS: u16 = 20;
pub const MADE_BY_UNIX: u16 = (3 << 8) | 20;

pub fn dos_date(year: u16, month: u16, day: u16) -> u16 {
    ((year - 1980) << 9) | (month << 5) | day
}

pub fn dos_time(hour: u16, minute: u16, second: u16) -> u16 {
    (hour << 11) | (minute << 5) | (second / 2)
}

/// CRC-32 (IEEE), bitwise. Only used on small buffers.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Raw deflate made of stored blocks (no compression; valid deflate).
pub fn deflate_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
        return out;
    }
    let mut chunks = data.chunks(65_535).peekable();
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        out.push(if last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// One archive entry. Every field can be forged.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Name bytes stored in the CENTRAL directory (and in the local header
    /// unless `local_name` is set).
    pub name: Vec<u8>,
    /// Forged local-header name (R-361-1 / AC-365-1).
    pub local_name: Option<Vec<u8>>,
    pub method: u16,
    pub flags: u16,
    /// Bytes stored as the entry data.
    pub payload: Vec<u8>,
    pub crc32: u32,
    /// Sizes written in the headers (default: real sizes).
    pub declared_uncompressed: u64,
    pub declared_compressed: u64,
    pub external_attrs: u32,
    pub made_by: u16,
    pub dos_date: u16,
    pub dos_time: u16,
    pub force_zip64: bool,
    pub extra_central: Vec<u8>,
    pub extra_local: Vec<u8>,
}

impl Entry {
    fn base(name: &[u8], method: u16, payload: Vec<u8>, uncompressed: u64, crc: u32) -> Entry {
        let flags = if std::str::from_utf8(name)
            .map(|s| !s.is_ascii())
            .unwrap_or(false)
        {
            FLAG_UTF8
        } else {
            0
        };
        Entry {
            name: name.to_vec(),
            local_name: None,
            method,
            flags,
            declared_compressed: payload.len() as u64,
            payload,
            crc32: crc,
            declared_uncompressed: uncompressed,
            external_attrs: 0x20,
            made_by: MADE_BY_DOS,
            dos_date: dos_date(2021, 5, 6),
            dos_time: dos_time(7, 8, 10),
            force_zip64: false,
            extra_central: Vec::new(),
            extra_local: Vec::new(),
        }
    }

    /// Deflate entry (stored blocks). Non-ASCII UTF-8 names get bit 11.
    pub fn file(name: &str, data: &[u8]) -> Entry {
        Entry::base(
            name.as_bytes(),
            METHOD_DEFLATE,
            deflate_stored(data),
            data.len() as u64,
            crc32(data),
        )
    }

    /// Stored entry.
    pub fn stored(name: &str, data: &[u8]) -> Entry {
        Entry::base(
            name.as_bytes(),
            METHOD_STORED,
            data.to_vec(),
            data.len() as u64,
            crc32(data),
        )
    }

    /// Directory entry (a trailing `/` is appended when missing).
    pub fn dir(name: &str) -> Entry {
        let mut n = name.to_string();
        if !n.ends_with('/') {
            n.push('/');
        }
        let mut e = Entry::base(n.as_bytes(), METHOD_STORED, Vec::new(), 0, 0);
        e.external_attrs = 0x10;
        e
    }

    /// Symlink entry as macOS/Linux tools write it (Unix host, `S_IFLNK`,
    /// stored target path as data) — R-362 #6 / AC-367.
    pub fn symlink(name: &str, target: &str) -> Entry {
        let mut e = Entry::stored(name, target.as_bytes());
        e.made_by = MADE_BY_UNIX;
        e.external_attrs = 0o120777 << 16;
        e
    }

    /// AES-encrypted entry as written by common tools: method 99, bit 0 set and
    /// the 0x9901 extra field. `with_bit0 = false` builds the variant that only
    /// signals encryption through method 99 (SPEC-0200-0001 §3.4.2).
    pub fn aes(name: &str, with_bit0: bool) -> Entry {
        let mut e = Entry::base(name.as_bytes(), METHOD_AES, vec![0u8; 24], 5, 0);
        if with_bit0 {
            e.flags |= FLAG_ENCRYPTED;
        }
        // 0x9901: vendor version 2, "AE", strength 3 (256 bit), real method 8.
        let extra = vec![
            0x01, 0x99, 0x07, 0x00, 0x02, 0x00, b'A', b'E', 0x03, 0x08, 0x00,
        ];
        e.extra_central = extra.clone();
        e.extra_local = extra;
        e
    }

    /// An entry whose HEADERS declare `uncompressed` / `compressed` bytes and
    /// whose data is `compressed` zero bytes (physically consistent, content
    /// garbage): for L-2 / L-5 style tests that never decompress.
    pub fn declared(name: &str, method: u16, uncompressed: u64, compressed: u64) -> Entry {
        let mut e = Entry::base(
            name.as_bytes(),
            method,
            vec![0u8; compressed as usize],
            uncompressed,
            0,
        );
        e.declared_compressed = compressed;
        e
    }

    // ---- builder-style forgeries ----
    pub fn name_bytes(mut self, name: &[u8]) -> Entry {
        self.name = name.to_vec();
        self
    }
    pub fn local_name(mut self, name: &[u8]) -> Entry {
        self.local_name = Some(name.to_vec());
        self
    }
    pub fn flags(mut self, flags: u16) -> Entry {
        self.flags = flags;
        self
    }
    pub fn set_flag(mut self, flag: u16) -> Entry {
        self.flags |= flag;
        self
    }
    pub fn clear_flag(mut self, flag: u16) -> Entry {
        self.flags &= !flag;
        self
    }
    pub fn method(mut self, method: u16) -> Entry {
        self.method = method;
        self
    }
    pub fn crc(mut self, crc: u32) -> Entry {
        self.crc32 = crc;
        self
    }
    /// Forges the sizes written into the headers.
    pub fn declare_sizes(mut self, uncompressed: u64, compressed: u64) -> Entry {
        self.declared_uncompressed = uncompressed;
        self.declared_compressed = compressed;
        self
    }
    pub fn payload(mut self, payload: Vec<u8>) -> Entry {
        self.declared_compressed = payload.len() as u64;
        self.payload = payload;
        self
    }
    /// Unix host + mode (`0o100644` for a regular file, `0o120777` for a link).
    pub fn unix_mode(mut self, mode: u32) -> Entry {
        self.made_by = MADE_BY_UNIX;
        self.external_attrs = mode << 16;
        self
    }
    /// MS-DOS attribute bits (`0x01` read-only, `0x02` hidden, `0x10` dir ...).
    pub fn dos_attrs(mut self, attrs: u32) -> Entry {
        self.made_by = MADE_BY_DOS;
        self.external_attrs = attrs;
        self
    }
    pub fn dos_datetime(mut self, date: u16, time: u16) -> Entry {
        self.dos_date = date;
        self.dos_time = time;
        self
    }
    pub fn zip64(mut self) -> Entry {
        self.force_zip64 = true;
        self
    }
    pub fn extra_central(mut self, extra: &[u8]) -> Entry {
        self.extra_central.extend_from_slice(extra);
        self
    }
    pub fn extra_local(mut self, extra: &[u8]) -> Entry {
        self.extra_local.extend_from_slice(extra);
        self
    }

    fn needs_zip64(&self) -> bool {
        self.force_zip64
            || self.declared_uncompressed >= 0xFFFF_FFFF
            || self.declared_compressed >= 0xFFFF_FFFF
    }
}

/// Info-ZIP Unicode Path extra field (0x7075) for a header name (F5 / F10).
/// `crc_of` is the CRC-32 recorded for the header name; pass a wrong value to
/// forge a mismatch.
pub fn unicode_path_extra(crc_of: u32, utf8_name: &str) -> Vec<u8> {
    let mut extra = Vec::new();
    put16(&mut extra, 0x7075);
    put16(&mut extra, (1 + 4 + utf8_name.len()) as u16);
    extra.push(1);
    put32(&mut extra, crc_of);
    extra.extend_from_slice(utf8_name.as_bytes());
    extra
}

/// The archive under construction.
#[derive(Debug, Clone, Default)]
pub struct ZipBuilder {
    entries: Vec<Entry>,
    comment: Vec<u8>,
    disk_number: u16,
    cd_disk: u16,
    declared_total: Option<u64>,
    declared_this_disk: Option<u64>,
    force_zip64_eocd: bool,
    zip64_total_disks: Option<u32>,
}

impl ZipBuilder {
    pub fn new() -> ZipBuilder {
        ZipBuilder::default()
    }
    pub fn entry(mut self, entry: Entry) -> ZipBuilder {
        self.entries.push(entry);
        self
    }
    pub fn file(self, name: &str, data: &[u8]) -> ZipBuilder {
        self.entry(Entry::file(name, data))
    }
    pub fn dir(self, name: &str) -> ZipBuilder {
        self.entry(Entry::dir(name))
    }
    pub fn comment(mut self, comment: &[u8]) -> ZipBuilder {
        self.comment = comment.to_vec();
        self
    }
    /// EOCD "number of this disk" and "disk where the central directory starts".
    pub fn disks(mut self, this_disk: u16, cd_disk: u16) -> ZipBuilder {
        self.disk_number = this_disk;
        self.cd_disk = cd_disk;
        self
    }
    /// Forges the total entry count written in the EOCD.
    pub fn declare_entry_count(mut self, total: u64) -> ZipBuilder {
        self.declared_total = Some(total);
        self
    }
    /// Forges the "entries on this disk" count (default = total).
    pub fn declare_entries_on_this_disk(mut self, n: u64) -> ZipBuilder {
        self.declared_this_disk = Some(n);
        self
    }
    /// Always write the ZIP64 EOCD record + locator.
    pub fn force_zip64_eocd(mut self) -> ZipBuilder {
        self.force_zip64_eocd = true;
        self
    }
    /// Forges the "total number of disks" of the ZIP64 locator.
    pub fn zip64_total_disks(mut self, n: u32) -> ZipBuilder {
        self.zip64_total_disks = Some(n);
        self
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Serializes the archive.
    pub fn build(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut offsets = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            offsets.push(out.len() as u64);
            let zip64 = e.needs_zip64();
            let local_name = e.local_name.as_ref().unwrap_or(&e.name);
            let mut extra = Vec::new();
            if zip64 {
                put16(&mut extra, 0x0001);
                put16(&mut extra, 16);
                put64(&mut extra, e.declared_uncompressed);
                put64(&mut extra, e.declared_compressed);
            }
            extra.extend_from_slice(&e.extra_local);
            put32(&mut out, 0x0403_4b50);
            put16(&mut out, if zip64 { 45 } else { 20 });
            put16(&mut out, e.flags);
            put16(&mut out, e.method);
            put16(&mut out, e.dos_time);
            put16(&mut out, e.dos_date);
            put32(&mut out, e.crc32);
            put32(
                &mut out,
                if zip64 {
                    0xFFFF_FFFF
                } else {
                    e.declared_compressed as u32
                },
            );
            put32(
                &mut out,
                if zip64 {
                    0xFFFF_FFFF
                } else {
                    e.declared_uncompressed as u32
                },
            );
            put16(&mut out, local_name.len() as u16);
            put16(&mut out, extra.len() as u16);
            out.extend_from_slice(local_name);
            out.extend_from_slice(&extra);
            out.extend_from_slice(&e.payload);
        }

        let cd_start = out.len() as u64;
        for (e, offset) in self.entries.iter().zip(&offsets) {
            let zip64 = e.needs_zip64();
            let mut extra = Vec::new();
            if zip64 {
                put16(&mut extra, 0x0001);
                put16(&mut extra, 16);
                put64(&mut extra, e.declared_uncompressed);
                put64(&mut extra, e.declared_compressed);
            }
            extra.extend_from_slice(&e.extra_central);
            put32(&mut out, 0x0201_4b50);
            put16(&mut out, e.made_by);
            put16(&mut out, if zip64 { 45 } else { 20 });
            put16(&mut out, e.flags);
            put16(&mut out, e.method);
            put16(&mut out, e.dos_time);
            put16(&mut out, e.dos_date);
            put32(&mut out, e.crc32);
            put32(
                &mut out,
                if zip64 {
                    0xFFFF_FFFF
                } else {
                    e.declared_compressed as u32
                },
            );
            put32(
                &mut out,
                if zip64 {
                    0xFFFF_FFFF
                } else {
                    e.declared_uncompressed as u32
                },
            );
            put16(&mut out, e.name.len() as u16);
            put16(&mut out, extra.len() as u16);
            put16(&mut out, 0); // comment length
            put16(&mut out, 0); // disk number start
            put16(&mut out, 0); // internal attributes
            put32(&mut out, e.external_attrs);
            put32(&mut out, *offset as u32);
            out.extend_from_slice(&e.name);
            out.extend_from_slice(&extra);
        }
        let cd_size = out.len() as u64 - cd_start;

        let total = self.declared_total.unwrap_or(self.entries.len() as u64);
        let this_disk = self.declared_this_disk.unwrap_or(total);
        let zip64_eocd = self.force_zip64_eocd || total >= 0xFFFF || this_disk >= 0xFFFF;
        if zip64_eocd {
            let record_offset = out.len() as u64;
            put32(&mut out, 0x0606_4b50);
            put64(&mut out, 44);
            put16(&mut out, 45);
            put16(&mut out, 45);
            put32(&mut out, self.disk_number as u32);
            put32(&mut out, self.cd_disk as u32);
            put64(&mut out, this_disk);
            put64(&mut out, total);
            put64(&mut out, cd_size);
            put64(&mut out, cd_start);
            put32(&mut out, 0x0706_4b50);
            put32(&mut out, self.disk_number as u32);
            put64(&mut out, record_offset);
            put32(&mut out, self.zip64_total_disks.unwrap_or(1));
        }
        put32(&mut out, 0x0605_4b50);
        put16(&mut out, self.disk_number);
        put16(&mut out, self.cd_disk);
        put16(&mut out, if zip64_eocd { 0xFFFF } else { this_disk as u16 });
        put16(&mut out, if zip64_eocd { 0xFFFF } else { total as u16 });
        put32(
            &mut out,
            if zip64_eocd {
                0xFFFF_FFFF
            } else {
                cd_size as u32
            },
        );
        put32(
            &mut out,
            if zip64_eocd {
                0xFFFF_FFFF
            } else {
                cd_start as u32
            },
        );
        put16(&mut out, self.comment.len() as u16);
        out.extend_from_slice(&self.comment);
        out
    }

    /// Writes the archive to `dir/name` and returns the path.
    pub fn write_to(&self, dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, self.build())
            .unwrap_or_else(|e| panic!("write fixture {}: {e}", path.display()));
        path
    }
}

/// One central-directory record as an independent parser sees it (used to
/// inspect archives produced by `create_zip` without trusting the `zip` crate).
#[derive(Debug, Clone)]
pub struct CdEntry {
    pub name: Vec<u8>,
    pub flags: u16,
    pub method: u16,
    pub crc32: u32,
    pub compressed: u64,
    pub uncompressed: u64,
    pub made_by: u16,
    pub external_attrs: u32,
    pub dos_date: u16,
    pub dos_time: u16,
    pub extra: Vec<u8>,
}

impl CdEntry {
    pub fn name_str(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }
}

fn rd16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn rd32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
fn rd64(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

/// Parses the central directory of `bytes` (handles the ZIP64 EOCD record for
/// the entry count / directory offset; per-entry ZIP64 extras are not needed for
/// the archives the tests create).
pub fn parse_central_directory(bytes: &[u8]) -> Vec<CdEntry> {
    let eocd = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| rd32(bytes, i) == 0x0605_4b50)
        .expect("EOCD signature");
    let mut total = rd16(bytes, eocd + 10) as u64;
    let mut cd_offset = rd32(bytes, eocd + 16) as u64;
    if total == 0xFFFF || cd_offset == 0xFFFF_FFFF {
        let rec = (0..eocd)
            .rev()
            .find(|&i| rd32(bytes, i) == 0x0606_4b50)
            .expect("ZIP64 EOCD record");
        total = rd64(bytes, rec + 32);
        cd_offset = rd64(bytes, rec + 48);
    }
    let mut at = cd_offset as usize;
    let mut out = Vec::new();
    for _ in 0..total {
        assert_eq!(
            rd32(bytes, at),
            0x0201_4b50,
            "central directory signature at {at}"
        );
        let name_len = rd16(bytes, at + 28) as usize;
        let extra_len = rd16(bytes, at + 30) as usize;
        let comment_len = rd16(bytes, at + 32) as usize;
        out.push(CdEntry {
            made_by: rd16(bytes, at + 4),
            flags: rd16(bytes, at + 8),
            method: rd16(bytes, at + 10),
            dos_time: rd16(bytes, at + 12),
            dos_date: rd16(bytes, at + 14),
            crc32: rd32(bytes, at + 16),
            compressed: rd32(bytes, at + 20) as u64,
            uncompressed: rd32(bytes, at + 24) as u64,
            external_attrs: rd32(bytes, at + 38),
            name: bytes[at + 46..at + 46 + name_len].to_vec(),
            extra: bytes[at + 46 + name_len..at + 46 + name_len + extra_len].to_vec(),
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    out
}
