//! `archive/win_fs.rs` — the ONLY module allowed to contain `unsafe`
//! (SPEC-0200-0001 §3.4.13-7, §3.4.14 C2): `MoveFileExW` (without
//! `MOVEFILE_REPLACE_EXISTING`) and `GetDiskFreeSpaceExW`, each call with a
//! `// SAFETY:` comment. `lib.rs` carries `#![deny(unsafe_code)]`; this file
//! opts out with `#![allow(unsafe_code)]`.
//!
//! Non-Windows builds (test environments only) use `hard_link` +
//! `remove_file` for the no-replace move and report `free_space` as
//! unsupported (callers then skip L-3, R-365-6). Tests:
//! `archive/win_fs_tests.rs`.
#![allow(unsafe_code)]

use std::io;
use std::path::Path;

/// MUST-66: atomically moves `from` to `to` WITHOUT replacing. If `to` exists
/// (any kind) the result is `Err` with `ErrorKind::AlreadyExists`
/// (`ERROR_ALREADY_EXISTS` / `ERROR_FILE_EXISTS`) and both paths are left
/// untouched.
pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    imp::move_no_replace(from, to)
}

/// L-3 (R-365): bytes available to the caller on the volume of `path` (an
/// existing path). Errors are returned as-is (callers skip L-3, R-365-6).
pub fn free_space(path: &Path) -> io::Result<u64> {
    imp::free_space(path)
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, MoveFileExW};

    /// NUL-terminated UTF-16. A long absolute path gets the verbatim prefix
    /// (the Win32 API otherwise limits it to MAX_PATH, unlike Rust's std).
    fn wide(path: &Path) -> io::Result<Vec<u16>> {
        let mut text: Vec<u16> = path.as_os_str().encode_wide().collect();
        if text.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL character"));
        }
        let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
        if text.len() >= 240 && !text.starts_with(&verbatim) && path.is_absolute() {
            let unc: Vec<u16> = r"\\".encode_utf16().collect();
            if text.starts_with(&unc) {
                let mut prefixed: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
                prefixed.extend_from_slice(&text[2..]);
                text = prefixed;
            } else {
                let mut prefixed = verbatim;
                prefixed.extend_from_slice(&text);
                text = prefixed;
            }
        }
        text.push(0);
        Ok(text)
    }

    pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        let from_w = wide(from)?;
        let to_w = wide(to)?;
        // SAFETY: both pointers reference NUL-terminated UTF-16 buffers that
        // live until the call returns; flags 0 = no MOVEFILE_REPLACE_EXISTING,
        // so an existing destination makes the call fail (never replace).
        let ok = unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), 0) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn free_space(path: &Path) -> io::Result<u64> {
        let path_w = wide(path)?;
        let mut available: u64 = 0;
        // SAFETY: `path_w` is a NUL-terminated UTF-16 buffer that outlives the
        // call; `available` is a valid writable u64; the two optional output
        // pointers are null, which the API permits.
        let ok = unsafe { GetDiskFreeSpaceExW(path_w.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(available)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;
    use std::path::Path;

    pub fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
        std::fs::hard_link(from, to)?;
        std::fs::remove_file(from)
    }

    pub fn free_space(_path: &Path) -> io::Result<u64> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
