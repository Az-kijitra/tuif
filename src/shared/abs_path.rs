//! The one backend "is this an absolute path" judgement (SPEC-0200-0001 §3.12.2,
//! SPEC-0300-0011 R-89-6, SPEC-0300-0016 R-722-3).
//!
//! Tauri-free, no side effects, never converts. `is_absolute_path` is NOT `Path::is_absolute`
//! (they differ for `\\.\pipe\x`, `C:/foo`, `\\server`). The TypeScript counterpart is
//! `path-bar/normalize.ts`; the two cannot share code, the tests hold the same representative values.

/// R-722-3 / R-15-5: 32,767 UTF-16 units.
pub const MAX_PATH_UTF16: usize = 32_767;

/// The code string of every string-shaped `invalid_path` refusal.
pub const INVALID_PATH_CODE: &str = "invalid_path";

fn is_drive_prefix(chars: &[char]) -> bool {
    chars.len() >= 3 && chars[0].is_ascii_alphabetic() && chars[1] == ':' && chars[2] == '\\'
}

/// `Volume{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`-shaped (braces around a GUID).
fn is_volume_guid(component: &str) -> bool {
    let Some(rest) = component.strip_prefix("Volume") else {
        return false;
    };
    let Some(inner) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else {
        return false;
    };
    let groups: Vec<&str> = inner.split('-').collect();
    let lens = [8usize, 4, 4, 4, 12];
    groups.len() == 5
        && groups
            .iter()
            .zip(lens)
            .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// R-89-6 / SPEC-0300-0016 R-722-3: absolute-path forms only. Never converts.
pub fn is_absolute_path(path: &str) -> bool {
    if path.is_empty() || path.encode_utf16().count() > MAX_PATH_UTF16 {
        return false;
    }
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        return !rest.is_empty();
    }
    if let Some(rest) = path.strip_prefix(r"\\.\") {
        let chars: Vec<char> = rest.chars().collect();
        if is_drive_prefix(&chars) {
            return true;
        }
        let first = rest.split('\\').next().unwrap_or("");
        // `\\.\Volume{GUID}\...`: the trailing `\` must exist (otherwise it names the device itself).
        return rest.len() > first.len() && is_volume_guid(first);
    }
    if let Some(rest) = path.strip_prefix(r"\\") {
        // UNC: \\server\share[\...] -- a non-empty server and a non-empty share.
        let mut parts = rest.split('\\');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        return !server.is_empty() && !share.is_empty();
    }
    let chars: Vec<char> = path.chars().collect();
    is_drive_prefix(&chars)
}

/// For a RESOLVED `PathBuf` (ZIP, T-13). A non-UTF-8 path is `false`.
pub fn is_absolute_path_buf(path: &std::path::Path) -> bool {
    path.to_str().is_some_and(is_absolute_path)
}

/// The first path that is not absolute (for the R-89-2 message); `None` for an empty input.
pub fn first_relative<'a, I: IntoIterator<Item = &'a str>>(paths: I) -> Option<&'a str> {
    paths.into_iter().find(|p| !is_absolute_path(p))
}
