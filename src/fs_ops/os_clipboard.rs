//! `fs_ops/os_clipboard.rs` -- Windows Explorer clipboard integration.
//!
//! Provides bidirectional copy/cut/paste interoperability between `tuif` and
//! Windows Explorer using `CF_HDROP` and `Preferred DropEffect`.

#![allow(unsafe_code)]

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardEffect {
    Copy,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsClipboardPayload {
    pub paths: Vec<PathBuf>,
    pub effect: ClipboardEffect,
}

/// Builds a binary buffer adhering to the Win32 `DROPFILES` specification.
///
/// Layout:
/// - Offset 0..4:   `pFiles` (u32, little-endian) = 20
/// - Offset 4..8:   `pt.x` (i32, little-endian) = 0
/// - Offset 8..12:  `pt.y` (i32, little-endian) = 0
/// - Offset 12..16: `fNC` (i32, little-endian) = 0
/// - Offset 16..20: `fWide` (i32, little-endian) = 1 (Unicode UTF-16)
/// - Offset 20..:   Sequence of null-terminated UTF-16 strings, followed by a final null.
pub fn build_hdrop_buffer(paths: &[PathBuf]) -> Vec<u8> {
    if paths.is_empty() {
        return Vec::new();
    }

    let mut wide_chars: Vec<u16> = Vec::new();
    for p in paths {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            wide_chars.extend(p.as_os_str().encode_wide());
        }
        #[cfg(not(windows))]
        {
            wide_chars.extend(p.to_string_lossy().encode_utf16());
        }
        wide_chars.push(0); // Null terminator for each path
    }
    wide_chars.push(0); // Extra null terminator for the entire list

    let header_size: u32 = 20;
    let mut buffer = Vec::with_capacity(20 + wide_chars.len() * 2);

    // pFiles = 20
    buffer.extend_from_slice(&header_size.to_le_bytes());
    // pt.x = 0
    buffer.extend_from_slice(&0i32.to_le_bytes());
    // pt.y = 0
    buffer.extend_from_slice(&0i32.to_le_bytes());
    // fNC = 0
    buffer.extend_from_slice(&0i32.to_le_bytes());
    // fWide = 1
    buffer.extend_from_slice(&1i32.to_le_bytes());

    for w in wide_chars {
        buffer.extend_from_slice(&w.to_le_bytes());
    }

    buffer
}

/// Parses a binary buffer containing a Win32 `DROPFILES` structure.
pub fn parse_hdrop_buffer(data: &[u8]) -> Vec<PathBuf> {
    if data.len() < 20 {
        return Vec::new();
    }

    let p_files = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let f_wide = i32::from_le_bytes([data[16], data[17], data[18], data[19]]) != 0;

    if p_files > data.len() {
        return Vec::new();
    }

    let payload = &data[p_files..];
    let mut results = Vec::new();

    if f_wide {
        let u16_count = payload.len() / 2;
        let mut wide_data: Vec<u16> = Vec::with_capacity(u16_count);
        for i in 0..u16_count {
            let b0 = payload[i * 2];
            let b1 = payload[i * 2 + 1];
            wide_data.push(u16::from_le_bytes([b0, b1]));
        }

        let mut current_path: Vec<u16> = Vec::new();
        for &w in &wide_data {
            if w == 0 {
                if current_path.is_empty() {
                    // Double null reached: end of file list
                    break;
                }
                #[cfg(windows)]
                {
                    use std::ffi::OsString;
                    use std::os::windows::ffi::OsStringExt;
                    let s = OsString::from_wide(&current_path);
                    results.push(PathBuf::from(s));
                }
                #[cfg(not(windows))]
                {
                    let s = String::from_utf16_lossy(&current_path);
                    results.push(PathBuf::from(s));
                }
                current_path.clear();
            } else {
                current_path.push(w);
            }
        }
    } else {
        // ANSI fallback (rare, but supported)
        let mut current_path: Vec<u8> = Vec::new();
        for &b in payload {
            if b == 0 {
                if current_path.is_empty() {
                    break;
                }
                let s = String::from_utf8_lossy(&current_path).into_owned();
                results.push(PathBuf::from(s));
                current_path.clear();
            } else {
                current_path.push(b);
            }
        }
    }

    results
}

/// Reads file paths and the copy/cut mode from the OS clipboard.
pub fn get_os_clipboard_files() -> Option<OsClipboardPayload> {
    imp::get_os_clipboard_files()
}

/// Writes file paths and the copy/cut mode to the OS clipboard.
pub fn set_os_clipboard_files(paths: &[PathBuf], effect: ClipboardEffect) -> bool {
    imp::set_os_clipboard_files(paths, effect)
}

/// Clears the OS clipboard.
pub fn clear_os_clipboard() -> bool {
    imp::clear_os_clipboard()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{GlobalFree, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };

    static CLIPBOARD_LOCK: Mutex<()> = Mutex::new(());

    const CF_HDROP: u32 = 15;
    const DROPEFFECT_COPY: u32 = 1;
    const DROPEFFECT_MOVE: u32 = 2;

    fn get_preferred_drop_effect_format() -> u32 {
        // "Preferred DropEffect\0" in UTF-16
        static FORMAT_NAME: &[u16] = &[
            'P' as u16, 'r' as u16, 'e' as u16, 'f' as u16, 'e' as u16, 'r' as u16,
            'r' as u16, 'e' as u16, 'd' as u16, ' ' as u16, 'D' as u16, 'r' as u16,
            'o' as u16, 'p' as u16, 'E' as u16, 'f' as u16, 'f' as u16, 'e' as u16,
            'c' as u16, 't' as u16, 0,
        ];
        unsafe { RegisterClipboardFormatW(FORMAT_NAME.as_ptr()) }
    }

    struct ClipboardGuard<'a> {
        opened: bool,
        _lock: Option<std::sync::MutexGuard<'a, ()>>,
    }

    impl<'a> ClipboardGuard<'a> {
        fn open() -> Option<Self> {
            let guard = CLIPBOARD_LOCK.lock().ok()?;
            // Attempt to open clipboard with retry (up to 20 attempts with 10ms sleep)
            for _ in 0..20 {
                let ok = unsafe { OpenClipboard(0 as HWND) };
                if ok != 0 {
                    return Some(ClipboardGuard {
                        opened: true,
                        _lock: Some(guard),
                    });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        }
    }

    impl<'a> Drop for ClipboardGuard<'a> {
        fn drop(&mut self) {
            if self.opened {
                unsafe {
                    CloseClipboard();
                }
            }
        }
    }

    pub fn get_os_clipboard_files() -> Option<OsClipboardPayload> {
        let _guard = ClipboardGuard::open()?;

        if unsafe { IsClipboardFormatAvailable(CF_HDROP) } == 0 {
            return None;
        }

        let h_data = unsafe { GetClipboardData(CF_HDROP) };
        if h_data.is_null() {
            return None;
        }

        let size = unsafe { GlobalSize(h_data) };
        if size < 20 {
            return None;
        }

        let ptr = unsafe { GlobalLock(h_data) };
        if ptr.is_null() {
            return None;
        }

        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) };
        let paths = parse_hdrop_buffer(bytes);
        unsafe { GlobalUnlock(h_data) };

        if paths.is_empty() {
            return None;
        }

        // Query Preferred DropEffect
        let effect_format = get_preferred_drop_effect_format();
        let mut effect = ClipboardEffect::Copy;
        if effect_format != 0 && unsafe { IsClipboardFormatAvailable(effect_format) } != 0 {
            let h_effect = unsafe { GetClipboardData(effect_format) };
            if !h_effect.is_null() {
                let e_size = unsafe { GlobalSize(h_effect) };
                if e_size >= 4 {
                    let e_ptr = unsafe { GlobalLock(h_effect) };
                    if !e_ptr.is_null() {
                        let val = unsafe { *(e_ptr as *const u32) };
                        if val == DROPEFFECT_MOVE {
                            effect = ClipboardEffect::Move;
                        }
                        unsafe { GlobalUnlock(h_effect) };
                    }
                }
            }
        }

        Some(OsClipboardPayload { paths, effect })
    }

    pub fn set_os_clipboard_files(paths: &[PathBuf], effect: ClipboardEffect) -> bool {
        if paths.is_empty() {
            return false;
        }

        let hdrop_bytes = build_hdrop_buffer(paths);
        if hdrop_bytes.is_empty() {
            return false;
        }

        let _guard = match ClipboardGuard::open() {
            Some(g) => g,
            None => return false,
        };

        unsafe {
            if EmptyClipboard() == 0 {
                return false;
            }
        }

        // 1. Set CF_HDROP
        let h_mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, hdrop_bytes.len()) };
        if h_mem.is_null() {
            return false;
        }

        let dest_ptr = unsafe { GlobalLock(h_mem) };
        if dest_ptr.is_null() {
            unsafe { GlobalFree(h_mem) };
            return false;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(hdrop_bytes.as_ptr(), dest_ptr as *mut u8, hdrop_bytes.len());
            GlobalUnlock(h_mem);
        }

        let set_res = unsafe { SetClipboardData(CF_HDROP, h_mem) };
        if set_res.is_null() {
            unsafe { GlobalFree(h_mem) };
            return false;
        }
        // Once SetClipboardData succeeds, the system owns `h_mem`. Do not free it.

        // 2. Set Preferred DropEffect
        let effect_format = get_preferred_drop_effect_format();
        if effect_format != 0 {
            let effect_val: u32 = match effect {
                ClipboardEffect::Copy => DROPEFFECT_COPY,
                ClipboardEffect::Move => DROPEFFECT_MOVE,
            };

            let h_effect = unsafe { GlobalAlloc(GMEM_MOVEABLE, 4) };
            if !h_effect.is_null() {
                let e_ptr = unsafe { GlobalLock(h_effect) };
                if !e_ptr.is_null() {
                    unsafe {
                        *(e_ptr as *mut u32) = effect_val;
                        GlobalUnlock(h_effect);
                    }
                    let res = unsafe { SetClipboardData(effect_format, h_effect) };
                    if res.is_null() {
                        unsafe { GlobalFree(h_effect) };
                    }
                } else {
                    unsafe { GlobalFree(h_effect) };
                }
            }
        }

        true
    }

    pub fn clear_os_clipboard() -> bool {
        let _guard = match ClipboardGuard::open() {
            Some(g) => g,
            None => return false,
        };
        unsafe { EmptyClipboard() != 0 }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn get_os_clipboard_files() -> Option<OsClipboardPayload> {
        None
    }

    pub fn set_os_clipboard_files(_paths: &[PathBuf], _effect: ClipboardEffect) -> bool {
        false
    }

    pub fn clear_os_clipboard() -> bool {
        false
    }
}
