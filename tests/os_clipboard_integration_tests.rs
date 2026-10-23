//! Integration tests for Windows Explorer clipboard interoperability (`CF_HDROP`).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use cuifiler::app::AppState;
use cuifiler::fs_ops::os_clipboard::{
    clear_os_clipboard, get_os_clipboard_files, set_os_clipboard_files, ClipboardEffect,
};
use std::fs;
use std::sync::Mutex;

static TEST_CLIPBOARD_MUTEX: Mutex<()> = Mutex::new(());

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[cfg(windows)]
#[test]
fn test_tuif_yank_and_cut_exports_to_os_clipboard() {
    let _lock = TEST_CLIPBOARD_MUTEX.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let file_a = root.join("file_a.txt");
    fs::write(&file_a, "alpha").unwrap();

    let mut app = AppState::new(root.to_path_buf());
    while app.focused_entry().map(|e| e.name.as_str()) != Some("file_a.txt") {
        app.handle_key_event(key(KeyCode::Char('j')));
    }

    // 1. Yank (y) -> sets OS clipboard with Copy
    app.handle_key_event(key(KeyCode::Char('y')));
    let os_data = get_os_clipboard_files().expect("expected OS clipboard data on y");
    assert_eq!(os_data.effect, ClipboardEffect::Copy);
    assert_eq!(os_data.paths, vec![file_a.clone()]);

    // 2. Cut (x) -> sets OS clipboard with Move
    app.handle_key_event(key(KeyCode::Char('x')));
    let os_data_cut = get_os_clipboard_files().expect("expected OS clipboard data on x");
    assert_eq!(os_data_cut.effect, ClipboardEffect::Move);
    assert_eq!(os_data_cut.paths, vec![file_a.clone()]);

    clear_os_clipboard();
}

#[cfg(windows)]
#[test]
fn test_tuif_paste_imports_from_os_clipboard_copy() {
    let _lock = TEST_CLIPBOARD_MUTEX.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src_dir");
    let dest_dir = tmp.path().join("dest_dir");
    fs::create_dir(&src_dir).unwrap();
    fs::create_dir(&dest_dir).unwrap();

    let ext_file = src_dir.join("external_explorer_copy.txt");
    fs::write(&ext_file, "copied from explorer").unwrap();

    // Simulate Windows Explorer copying the file
    let ok = set_os_clipboard_files(&[ext_file.clone()], ClipboardEffect::Copy);
    assert!(ok);

    let mut app = AppState::new(dest_dir.clone());
    // Clear app's internal clipboard to verify it reads from OS clipboard
    app.clipboard = None;

    // Press 'p' to paste into dest_dir
    app.handle_key_event(key(KeyCode::Char('p')));
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.poll_active_transfer();
        if app.active_transfer.is_none() {
            break;
        }
    }

    let target_file = dest_dir.join("external_explorer_copy.txt");
    assert!(target_file.is_file(), "File should be pasted into dest_dir");
    assert_eq!(fs::read_to_string(&target_file).unwrap(), "copied from explorer");
    // Source file should still exist because it was a Copy
    assert!(ext_file.is_file());

    clear_os_clipboard();
}

#[cfg(windows)]
#[test]
fn test_tuif_paste_imports_from_os_clipboard_cut() {
    let _lock = TEST_CLIPBOARD_MUTEX.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let src_dir = tmp.path().join("src_dir");
    let dest_dir = tmp.path().join("dest_dir");
    fs::create_dir(&src_dir).unwrap();
    fs::create_dir(&dest_dir).unwrap();

    let ext_file = src_dir.join("external_explorer_cut.txt");
    fs::write(&ext_file, "cut from explorer").unwrap();

    // Simulate Windows Explorer cutting the file
    let ok = set_os_clipboard_files(&[ext_file.clone()], ClipboardEffect::Move);
    assert!(ok);

    let mut app = AppState::new(dest_dir.clone());
    app.clipboard = None;

    // Press 'p' to paste (move) into dest_dir
    app.handle_key_event(key(KeyCode::Char('p')));
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.poll_active_transfer();
        if app.active_transfer.is_none() {
            break;
        }
    }

    let target_file = dest_dir.join("external_explorer_cut.txt");
    assert!(target_file.is_file(), "File should be moved into dest_dir");
    assert_eq!(fs::read_to_string(&target_file).unwrap(), "cut from explorer");
    // Source file should NO LONGER exist because it was a Move (Cut)
    assert!(!ext_file.exists(), "Source file should have been moved");

    // After successful move, clipboard should be cleared
    assert!(get_os_clipboard_files().is_none(), "Clipboard should be cleared after move");
}
