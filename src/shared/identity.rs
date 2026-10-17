//! Entity-identity helpers (SPEC-0300-0011 R-25, R-37, R-37-1, R-37-2;
//! SPEC-0300-0014 R-325-1, R-343-2, R-367-1; SPEC-0200-0001 §3.3.13-2,
//! §3.4.1, §3.4.13-5 MUST-59 / MUST-69). Moved from `fs_ops/identity.rs`
//! (behavior unchanged) plus `normalize_allowing_missing`.
//!
//! Comparing paths as strings (`==`, `starts_with`) misses aliases of the same
//! entity: letter case on Windows, junctions/symlinks, verbatim (`\?\`)
//! spellings, 8.3 short names. Identity is therefore decided on
//! `std::fs::canonicalize` results (or, when a path cannot be canonicalized,
//! on the file ID `(dev, ino)` where the platform exposes it on stable Rust).
//!
//! `same_entity` / `is_strictly_within` return `Option<bool>`: `None` means
//! "identity could not be determined". Callers on a destructive path must
//! treat `None` as "do not proceed".

use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

// There is no stable std API for the Windows file index; canonicalize is the
// only identity source there.
#[cfg(not(unix))]
fn file_id(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// `Some(true)` when `a` and `b` are the very same filesystem entity.
pub(crate) fn same_entity(a: &Path, b: &Path) -> Option<bool> {
    if let (Ok(ca), Ok(cb)) = (fs::canonicalize(a), fs::canonicalize(b)) {
        return Some(ca == cb);
    }
    match (file_id(a), file_id(b)) {
        (Some(ia), Some(ib)) => Some(ia == ib),
        _ => None,
    }
}

/// `Some(true)` when `inner` lies strictly beneath `outer` (component-wise on
/// canonical paths, so `parent2` is not beneath `parent`). Equal entities are
/// `Some(false)`; use `same_entity` for those.
pub(crate) fn is_strictly_within(outer: &Path, inner: &Path) -> Option<bool> {
    let co = fs::canonicalize(outer).ok()?;
    let ci = fs::canonicalize(inner).ok()?;
    Some(ci != co && ci.starts_with(&co))
}

/// MUST-69 contract (SPEC-0200-0001 §3.4.13-5): canonical form of a path whose
/// tail may not exist yet.
/// (a) `path` must be absolute, otherwise `None`;
/// (b) split with `Path::components()` (never string concatenation);
/// (c) the LONGEST existing prefix (existence via `symlink_metadata`) is
///     canonicalized and the remainder, which may only consist of
///     `Component::Normal`, is appended; `.`/`..` in the remainder -> `None`
///     (a `..` inside the EXISTING prefix is resolved against the canonical
///     prefix);
/// (d) a prefix that exists but cannot be canonicalized (dangling link, access
///     denied) -> `None`.
pub(crate) fn normalize_allowing_missing(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut root = PathBuf::new();
    let mut rest_components = Vec::new();
    let mut in_root = true;
    for component in path.components() {
        if in_root {
            match component {
                Component::Prefix(_) | Component::RootDir => {
                    root.push(component.as_os_str());
                    continue;
                }
                _ => in_root = false,
            }
        }
        rest_components.push(component);
    }
    // The root (`C:\`, `/`) must exist; a missing drive has no existing prefix.
    let mut current = fs::canonicalize(&root).ok()?;
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    for component in rest_components {
        match component {
            Component::Normal(name) => {
                if !missing.is_empty() {
                    missing.push(name);
                    continue;
                }
                let candidate = current.join(name);
                match fs::symlink_metadata(&candidate) {
                    Ok(_) => current = fs::canonicalize(&candidate).ok()?,
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return None,
                    Err(_) => missing.push(name),
                }
            }
            Component::CurDir if missing.is_empty() => {}
            Component::ParentDir if missing.is_empty() => {
                if !current.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    for name in missing {
        current.push(name);
    }
    Some(current)
}
