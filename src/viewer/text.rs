//! `viewer/text.rs` (SPEC-0300-0015 R-445, R-445-1~4, R-450~R-454;
//! SPEC-0200-0001 §3.5.1 `read_text`). Reuses `encoding_rs` (already a
//! dependency, ZIP CP932 entry names) for the Shift_JIS decoder (R-450-3).
//!
//! Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! pub const MAX_TEXT_BYTES: u64 = 2_097_152;   // R-445
//! pub const MAX_TEXT_LINES: usize = 120_000;   // R-445-4
//!
//! // `bytes` is the (possibly already byte-truncated) content actually read;
//! // `truncated_by_size` says whether the caller stopped at MAX_TEXT_BYTES
//! // because the real file is larger (R-445-1/R-453 apply only when true).
//! pub fn decode(bytes: &[u8], truncated_by_size: bool) -> TextPayload; // R-450~R-454, R-445-4
//!
//! // Reads the file, applies the MAX_TEXT_BYTES cap, and calls `decode`;
//! // `original_size_bytes` on the result is always the REAL file size
//! // (R-445-2's {N} MiB / {B} bytes are computed from this, not from what
//! // was actually read).
//! pub fn read_text(path: &std::path::Path) -> std::io::Result<TextPayload>;
//!
//! // R-445-2: "ファイルサイズ {N} MiB({B} バイト)のうち先頭 2 MiB のみ表示して
//! // います" -- {N} rounded UP to one decimal place, {B} with digit-group
//! // separators. Returns None when `truncated_by_size` is false.
//! pub fn truncation_message(original_size_bytes: u64, truncated_by_size: bool) -> Option<String>;
//! ```
//!
//! Tests: `viewer/text_tests.rs`.

/// R-445: text (Markdown included) read limit, 2 MiB.
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
/// R-445-4: line-count display limit.
pub const MAX_TEXT_LINES: usize = 120_000;

/// R-451: the exact label strings shown in the header area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingLabel {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    ShiftJis,
}

impl EncodingLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            EncodingLabel::Utf8 => "UTF-8",
            EncodingLabel::Utf8Bom => "UTF-8 (BOM)",
            EncodingLabel::Utf16Le => "UTF-16LE",
            EncodingLabel::Utf16Be => "UTF-16BE",
            EncodingLabel::ShiftJis => "Shift_JIS",
        }
    }
}

/// R-445/R-450/R-460: what the frontend needs to render the text view.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPayload {
    pub content: String,
    pub encoding: EncodingLabel,
    /// The REAL file size (even when `truncated_by_size` cut the read short).
    pub original_size_bytes: u64,
    pub truncated_by_size: bool,
    pub truncated_by_lines: bool,
    pub line_count: usize,
}

// ---------------------------------------------------------------------
// R-450: encoding auto-detection + R-453/R-450-4 tail handling
// ---------------------------------------------------------------------

/// R-453(a): UTF-8 validity check that tolerates an incomplete trailing
/// multibyte sequence ONLY when `truncated` is true (the tail was cut by the
/// 2 MiB cap, not a genuine encoding error). Returns `None` when the bytes do
/// not count as UTF-8 (so the caller falls through to CP932, R-450 順5).
fn decode_utf8_tail_aware(bytes: &[u8], truncated: bool) -> Option<String> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Some(s.to_string()),
        Err(e) => {
            // `error_len() == None` means the error is an incomplete sequence
            // exactly at the end of `bytes` (would be valid with more input).
            if truncated && e.error_len().is_none() {
                let valid_up_to = e.valid_up_to();
                Some(std::str::from_utf8(&bytes[..valid_up_to]).expect("valid_up_to is always a UTF-8 boundary").to_string())
            } else {
                None
            }
        }
    }
}

/// R-453(b)/(c) + R-450-4: decodes `bytes` with a WHATWG decoder. When
/// `truncated` is true, uses the STREAMING decoder with `last = false`, which
/// buffers (drops, without an error) a trailing incomplete sequence instead
/// of reporting it — exactly R-453(b)/(c)'s "drop, no U+FFFD" rule, for both
/// UTF-16 (odd byte / unpaired surrogate) and Shift_JIS (dangling lead byte)
/// uniformly. When `truncated` is false, decodes as the final/only chunk, so
/// the SAME malformed tail becomes U+FFFD (R-450-4), matching the ordinary
/// WHATWG decode algorithm.
fn decode_with_encoding(encoding: &'static encoding_rs::Encoding, bytes: &[u8], truncated: bool) -> String {
    if truncated {
        let mut decoder = encoding.new_decoder_without_bom_handling();
        let mut output = String::with_capacity(bytes.len());
        let _ = decoder.decode_to_string(bytes, &mut output, false);
        output
    } else {
        let (cow, _had_errors) = encoding.decode_without_bom_handling(bytes);
        cow.into_owned()
    }
}

fn decode_encoding(bytes: &[u8], truncated: bool) -> (EncodingLabel, String) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let content =
            decode_utf8_tail_aware(rest, truncated).unwrap_or_else(|| String::from_utf8_lossy(rest).into_owned());
        return (EncodingLabel::Utf8Bom, content);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (EncodingLabel::Utf16Le, decode_with_encoding(encoding_rs::UTF_16LE, rest, truncated));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (EncodingLabel::Utf16Be, decode_with_encoding(encoding_rs::UTF_16BE, rest, truncated));
    }
    match decode_utf8_tail_aware(bytes, truncated) {
        Some(content) => (EncodingLabel::Utf8, content),
        None => (EncodingLabel::ShiftJis, decode_with_encoding(encoding_rs::SHIFT_JIS, bytes, truncated)),
    }
}

// ---------------------------------------------------------------------
// R-452: newline normalization + line counting/truncation (R-445-4, R-463)
// ---------------------------------------------------------------------

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// R-463: an empty string is 1 (empty) line. Otherwise: the number of `\n`
/// characters, plus 1 more if the text does not end with `\n` (a trailing
/// unterminated partial line).
fn count_lines(s: &str) -> usize {
    if s.is_empty() {
        return 1;
    }
    let newline_count = s.matches('\n').count();
    if s.ends_with('\n') {
        newline_count
    } else {
        newline_count + 1
    }
}

/// R-445-4: if `content` has more than `max_lines` lines, truncates to
/// exactly the first `max_lines` (cutting right after the `max_lines`-th
/// `\n`).
fn truncate_to_max_lines(content: String, max_lines: usize) -> (String, bool, usize) {
    let total = count_lines(&content);
    if total <= max_lines {
        return (content, false, total);
    }
    let mut seen = 0usize;
    let mut cut_at = content.len();
    for (i, c) in content.char_indices() {
        if c == '\n' {
            seen += 1;
            if seen == max_lines {
                cut_at = i + 1;
                break;
            }
        }
    }
    (content[..cut_at].to_string(), true, max_lines)
}

// ---------------------------------------------------------------------
// R-445-2: the truncation message wording/rounding
// ---------------------------------------------------------------------

fn format_with_commas(n: u64) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

fn ceil_to_one_decimal_mib(bytes: u64) -> f64 {
    let mib = bytes as f64 / (1024.0 * 1024.0);
    (mib * 10.0).ceil() / 10.0
}

/// R-445-2: "ファイルサイズ {N} MiB({B} バイト)のうち先頭 2 MiB のみ表示して
/// います" ({N} ceil to 1 decimal, {B} with digit-group separators). `None`
/// when not truncated.
pub fn truncation_message(original_size_bytes: u64, truncated_by_size: bool) -> Option<String> {
    if !truncated_by_size {
        return None;
    }
    let mib = ceil_to_one_decimal_mib(original_size_bytes);
    let bytes_str = format_with_commas(original_size_bytes);
    Some(format!("ファイルサイズ {mib:.1} MiB({bytes_str} バイト)のうち先頭 2 MiB のみ表示しています"))
}

// ---------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------

/// R-450~R-454, R-445-4: decodes an already-read byte buffer.
/// `truncated_by_size` says whether the caller stopped reading at
/// `MAX_TEXT_BYTES` because the real file is larger (R-445-1/R-453 apply only
/// when true).
pub fn decode(bytes: &[u8], truncated_by_size: bool) -> TextPayload {
    let (encoding, content) = decode_encoding(bytes, truncated_by_size);
    let normalized = normalize_newlines(&content);
    let (final_content, truncated_by_lines, line_count) = truncate_to_max_lines(normalized, MAX_TEXT_LINES);
    TextPayload {
        content: final_content,
        encoding,
        original_size_bytes: bytes.len() as u64,
        truncated_by_size,
        truncated_by_lines,
        line_count,
    }
}

/// Reads the file, applies the `MAX_TEXT_BYTES` cap, and decodes it.
/// `original_size_bytes` on the result is always the REAL file size.
pub fn read_text(path: &std::path::Path) -> std::io::Result<TextPayload> {
    use std::io::Read;

    let metadata = std::fs::metadata(path)?;
    let real_size = metadata.len();
    let truncated_by_size = real_size > MAX_TEXT_BYTES;
    let read_len = real_size.min(MAX_TEXT_BYTES) as usize;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; read_len];
    file.read_exact(&mut buf)?;

    let mut payload = decode(&buf, truncated_by_size);
    payload.original_size_bytes = real_size;
    Ok(payload)
}
