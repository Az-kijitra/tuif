//! `fs/win_drives.rs` -- logical-drive enumeration for the tree's top level
//! (SPEC-0300-0016 R-780/R-782/R-783/R-784/R-785; SPEC-0200-0001 §3.7.1).
//!
//! This module and `archive/win_fs.rs` are the only places allowed to contain
//! `unsafe` code (ISS-0161: the isolation was widened from one file to these
//! two; `lib.rs` keeps `#![deny(unsafe_code)]`, this file opts out below, and
//! `archive/boundary_tests.rs` pins the allow-list and the single call here).
//!
//! `bitmask_to_drive_roots` is a pure function (no OS call) tested on every
//! platform in `win_drives_tests.rs`; `list_logical_drives` is the thin OS
//! wrapper around it. R-783: only the bitmask is read -- no drive's contents
//! or volume information is touched, so nothing spins up, connects or shows
//! an OS dialog.
#![allow(unsafe_code)]

/// R-780/R-770-1: bit N (bit 0 = drive `A`) set in `mask` means `<letter>:\`
/// is present (exactly what `GetLogicalDrives` returns). Iterating the bits in
/// order yields ascending drive-letter order; bits 26-31 (past `Z`) are
/// ignored (SHOULD-139(e)).
pub fn bitmask_to_drive_roots(mask: u32) -> Vec<String> {
    (0u8..26)
        .filter(|bit| mask & (1u32 << bit) != 0)
        .map(|bit| format!("{}:\\", char::from(b'A' + bit)))
        .collect()
}

/// R-780/R-785 (method B): the drive roots the OS currently knows
/// (fixed, removable, network-mapped, optical, RAM disk, `subst`; including
/// not-ready and disconnected ones).
pub fn list_logical_drives() -> Vec<String> {
    imp::list_logical_drives()
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Storage::FileSystem::GetLogicalDrives;

    pub fn list_logical_drives() -> Vec<String> {
        // SAFETY: GetLogicalDrives takes no arguments and, on failure, returns
        // 0 (it has no error code of its own). The result is a plain u32 bit
        // mask owned by this process, so there are no pointers, lifetimes or
        // buffers to uphold invariants for.
        let mask = unsafe { GetLogicalDrives() };
        super::bitmask_to_drive_roots(mask)
    }
}

#[cfg(not(windows))]
mod imp {
    // Non-Windows builds (test environments only): there is no drive concept;
    // the frontend injects its own list for tests (TreeStateDeps.listDrives).
    pub fn list_logical_drives() -> Vec<String> {
        Vec::new()
    }
}
