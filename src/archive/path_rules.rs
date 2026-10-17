//! `archive/path_rules.rs` (SPEC-0300-0014 R-361 / R-363 / R-364 / R-324;
//! SPEC-0200-0001 §3.4.2, §3.4.9-a/b, §3.4.13-6, SHOULD-90/91/96).
//!
//! Pure functions: entry-name validation, path-length limits, the duplicate
//! detection key and the output file name check. The numeric limits are the
//! three constants below (§3.4.9-a: one place, change is local).
//! Tests: `archive/path_rules_tests.rs`.

use super::error::ArchiveErrorCode;
use std::ffi::OsStr;

/// R-363 #9: maximum full path length for a file (UTF-16 code units, without a
/// `\\?\` prefix).
pub const MAX_FILE_PATH_UTF16: usize = 259;
/// R-363 #9: maximum full path length for a directory (also applied to the
/// directories implied by an entry name).
pub const MAX_DIR_PATH_UTF16: usize = 247;
/// R-363 #10: maximum length of one path component.
pub const MAX_COMPONENT_UTF16: usize = 255;

/// Length in UTF-16 code units (`str::encode_utf16().count()`).
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn is_separator(c: char) -> bool {
    c == '/' || c == '\\'
}

/// R-361 + R-363 #1-#8, #10 on ONE entry name (already decoded per R-347).
///
/// * `\` and `/` both separate components; `.` components and empty components
///   (`a//b`) are dropped (R-363 note, SHOULD-91); a trailing separator (a
///   directory entry `d/`) yields the same components as `d`.
/// * A LEADING separator (absolute, UNC, `\\?\`, `\\.\`) is rejected.
/// * Errors are `ZipEntryRejected` for every rule (`..`, drive (`C:x`),
///   reserved device names, `:`, trailing dot/space, `<>"|?*` and control
///   characters, a name with no component at all, component > 255 UTF-16
///   units). R-363 #11 (unpaired surrogates) cannot occur in a `&str`; see
///   `has_unpaired_surrogate` and `entry_name::resolve_entry_name`.
pub fn validate_entry_name(name: &str) -> Result<Vec<String>, ArchiveErrorCode> {
    const REJECTED: ArchiveErrorCode = ArchiveErrorCode::ZipEntryRejected;
    if name.starts_with(is_separator) {
        return Err(REJECTED);
    }
    let mut components = Vec::new();
    for part in name.split(is_separator) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(REJECTED);
        }
        if utf16_len(part) > MAX_COMPONENT_UTF16 {
            return Err(REJECTED);
        }
        if part.chars().any(|c| c == ':' || matches!(c, '<' | '>' | '"' | '|' | '?' | '*') || (c as u32) < 0x20) {
            return Err(REJECTED);
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(REJECTED);
        }
        if is_reserved_device_name(part) {
            return Err(REJECTED);
        }
        components.push(part.to_string());
    }
    if components.is_empty() {
        return Err(REJECTED);
    }
    Ok(components)
}

/// R-363 #4: reserved Windows device name. Compared case-insensitively on the
/// part of the component BEFORE THE FIRST DOT with trailing spaces removed:
/// `CON PRN AUX NUL COM1-9 LPT1-9 CONIN$ CONOUT$` and the superscript forms
/// `COM¹ COM² COM³ LPT¹ LPT² LPT³`.
pub fn is_reserved_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or("").trim_end_matches(' ');
    const PLAIN: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
    if PLAIN.iter().any(|reserved| stem.eq_ignore_ascii_case(reserved)) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if stem.len() >= prefix.len()
            && stem.is_char_boundary(prefix.len())
            && stem[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            let suffix = &stem[prefix.len()..];
            let mut chars = suffix.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                if matches!(c, '1'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}') {
                    return true;
                }
            }
        }
    }
    false
}

/// R-363 #9 (+ SHOULD-90): checks `destination` (a path WITHOUT a trailing
/// separator, `\\?\` prefix tolerated and ignored; `\\?\UNC\srv\sh` counts as
/// `\\srv\sh`) joined with `components` by `\`. The final path may be at most
/// 259 (file) / 247 (directory) UTF-16 units, and EVERY implied parent
/// directory (`destination\c0`, `destination\c0\c1`, ...) at most 247.
/// Violations are `ZipEntryRejected`.
pub fn check_path_lengths(destination: &str, components: &[String], is_dir: bool) -> Result<(), ArchiveErrorCode> {
    let base = strip_verbatim_prefix(destination);
    let mut length = utf16_len(&base);
    let mut ends_with_separator = base.ends_with('\\') || base.ends_with('/');
    for (i, component) in components.iter().enumerate() {
        if !ends_with_separator {
            length += 1;
        }
        ends_with_separator = false;
        length += utf16_len(component);
        let is_last = i + 1 == components.len();
        let limit = if is_last && !is_dir { MAX_FILE_PATH_UTF16 } else { MAX_DIR_PATH_UTF16 };
        if length > limit {
            return Err(ArchiveErrorCode::ZipEntryRejected);
        }
    }
    Ok(())
}

/// R-364 / SHOULD-91: duplicate-detection key. Each component is folded by a
/// simple 1:1 uppercase (a char whose `to_uppercase()` yields several chars is
/// kept as is), no Unicode normalization; components joined with `/`.
pub fn fold_key(components: &[String]) -> String {
    let folded: Vec<String> = components
        .iter()
        .map(|component| {
            component
                .chars()
                .map(|c| {
                    let mut upper = c.to_uppercase();
                    match (upper.next(), upper.next()) {
                        (Some(u), None) => u,
                        _ => c,
                    }
                })
                .collect::<String>()
        })
        .collect();
    folded.join("/")
}

/// R-363 #11: `true` when the UTF-16 sequence has a lone (unpaired) surrogate.
pub fn has_unpaired_surrogate(units: &[u16]) -> bool {
    char::decode_utf16(units.iter().copied()).any(|r| r.is_err())
}

/// R-325 last row / SHOULD-96: a file name from the OS as UTF-8 for use as an
/// entry name; an unpaired surrogate is `InvalidPath` (never lossy-converted).
pub fn os_name_to_utf8(name: &OsStr) -> Result<String, ArchiveErrorCode> {
    name.to_str().map(str::to_string).ok_or(ArchiveErrorCode::InvalidPath)
}

/// R-324: the output file name check (SPEC-0300-0011 R-62 duplicated here so
/// `archive/` need not `use` `fs_ops`): empty/blank, `.` , `..` or one of the 9
/// characters `\ / : * ? " < > |` -> `InvalidName`.
pub fn validate_output_file_name(name: &str) -> Result<(), ArchiveErrorCode> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(ArchiveErrorCode::InvalidName);
    }
    if name.chars().any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err(ArchiveErrorCode::InvalidName);
    }
    Ok(())
}

/// SHOULD-90: strips a `\\?\` prefix (`\\?\C:\x` -> `C:\x`, `\\?\UNC\s\h\x` ->
/// `\\s\h\x`); other input is returned unchanged.
pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    path.to_string()
}

/// T-14 (SPEC-0200-0001 §3.12.5): `/` in the user's INPUT text of an output path / destination is a
/// separator like `\`. Nothing else changes. Applied to those two inputs only, never to `items`,
/// `zip_path` or `base_dir`.
pub fn normalize_input_separators(input: &str) -> String {
    input.replace('/', "\\")
}
