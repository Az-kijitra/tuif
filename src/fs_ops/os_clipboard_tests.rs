//! `fs_ops/os_clipboard_tests.rs` -- Unit tests for Win32 CF_HDROP clipboard buffer encoding/decoding.

use super::os_clipboard::{build_hdrop_buffer, parse_hdrop_buffer, ClipboardEffect};
use std::path::PathBuf;

#[test]
fn test_hdrop_buffer_empty() {
    let empty: Vec<PathBuf> = Vec::new();
    let buf = build_hdrop_buffer(&empty);
    assert!(buf.is_empty());
    let parsed = parse_hdrop_buffer(&buf);
    assert!(parsed.is_empty());
}

#[test]
fn test_hdrop_buffer_single_path_roundtrip() {
    let paths = vec![PathBuf::from(r"C:\Windows\System32\notepad.exe")];
    let buf = build_hdrop_buffer(&paths);
    assert!(buf.len() > 20);

    // Verify DROPFILES header
    assert_eq!(&buf[0..4], &20u32.to_le_bytes()); // pFiles = 20
    assert_eq!(&buf[16..20], &1i32.to_le_bytes()); // fWide = 1

    let parsed = parse_hdrop_buffer(&buf);
    assert_eq!(parsed, paths);
}

#[test]
fn test_hdrop_buffer_multiple_paths_roundtrip() {
    let paths = vec![
        PathBuf::from(r"C:\Users\test\Documents\file1.txt"),
        PathBuf::from(r"D:\Photos\vacation 2026\image.png"),
        PathBuf::from(r"E:\Projects\tuif\日本語\ドキュメント.md"),
    ];
    let buf = build_hdrop_buffer(&paths);
    let parsed = parse_hdrop_buffer(&buf);
    assert_eq!(parsed, paths);
}

#[test]
fn test_hdrop_buffer_corrupted_short_slice() {
    let short_buf = vec![0u8; 10];
    let parsed = parse_hdrop_buffer(&short_buf);
    assert!(parsed.is_empty());
}

#[test]
fn test_hdrop_buffer_corrupted_pfiles_offset() {
    let mut buf = vec![0u8; 30];
    buf[0..4].copy_from_slice(&100u32.to_le_bytes()); // pFiles = 100 > buf.len()
    let parsed = parse_hdrop_buffer(&buf);
    assert!(parsed.is_empty());
}

#[test]
fn test_hdrop_ansi_fallback() {
    // Manually construct an ANSI DROPFILES buffer (fWide = 0)
    let mut buf = Vec::new();
    buf.extend_from_slice(&20u32.to_le_bytes()); // pFiles = 20
    buf.extend_from_slice(&0i32.to_le_bytes()); // pt.x = 0
    buf.extend_from_slice(&0i32.to_le_bytes()); // pt.y = 0
    buf.extend_from_slice(&0i32.to_le_bytes()); // fNC = 0
    buf.extend_from_slice(&0i32.to_le_bytes()); // fWide = 0 (ANSI)
    buf.extend_from_slice(b"C:\\test\\file.txt\0\0");

    let parsed = parse_hdrop_buffer(&buf);
    assert_eq!(parsed, vec![PathBuf::from(r"C:\test\file.txt")]);
}

#[cfg(windows)]
#[test]
fn test_os_clipboard_live_roundtrip() {
    use super::os_clipboard::{clear_os_clipboard, get_os_clipboard_files, set_os_clipboard_files};

    let test_paths = vec![
        PathBuf::from(r"C:\temp\tuif_clipboard_test_1.txt"),
        PathBuf::from(r"C:\temp\tuif_clipboard_test_2.txt"),
    ];

    // Set as Copy
    let ok = set_os_clipboard_files(&test_paths, ClipboardEffect::Copy);
    assert!(ok, "set_os_clipboard_files failed");

    let payload = get_os_clipboard_files().expect("expected payload in clipboard");
    assert_eq!(payload.paths, test_paths);
    assert_eq!(payload.effect, ClipboardEffect::Copy);

    // Set as Move
    let ok_move = set_os_clipboard_files(&test_paths, ClipboardEffect::Move);
    assert!(ok_move, "set_os_clipboard_files move failed");

    let payload_move = get_os_clipboard_files().expect("expected move payload in clipboard");
    assert_eq!(payload_move.paths, test_paths);
    assert_eq!(payload_move.effect, ClipboardEffect::Move);

    // Clear
    let cleared = clear_os_clipboard();
    assert!(cleared, "clear_os_clipboard failed");
    assert!(get_os_clipboard_files().is_none());
}
