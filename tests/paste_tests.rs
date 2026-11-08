//! Characterization tests for paste (`y` / `x` then `p`): local copy and move, the conflict
//! prompt, and pastes that must not touch data (same folder, folder into itself).
//!
//! They pin the current behavior of `AppState::execute_paste` before it is split up. The SFTP
//! paths (upload, download, remote-to-remote) need a live server and are covered here only up
//! to the "no session" guards.
//!
//! `y` / `x` also write the Windows clipboard (`CF_HDROP`), and `p` reads it first, so every
//! test holds `OS_CLIPBOARD` and clears the clipboard when it is done.

mod common;

use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use common::{focus, fresh_app, key};
use crossterm::event::KeyCode;
use tuif::app::{
    AppState, ClipMode, ClipboardItem, FileClipboard, FocusPane, RemoteNodeInfo, TreeNode,
};
use tuif::fs_ops::os_clipboard::clear_os_clipboard;

static OS_CLIPBOARD: Mutex<()> = Mutex::new(());

/// Serializes access to the system clipboard and leaves it empty on drop.
struct ClipboardGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        clear_os_clipboard();
    }
}

fn lock_clipboard() -> ClipboardGuard {
    let guard = OS_CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    clear_os_clipboard();
    ClipboardGuard(guard)
}

/// What the background transfer is doing after `wait`.
#[derive(Debug, PartialEq, Eq)]
enum Waited {
    Finished,
    ConflictPrompt(String),
}

/// Polls the transfer until it finishes or asks about a conflict.
fn wait(app: &mut AppState) -> Waited {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_active_transfer();
        match app.active_transfer.as_ref() {
            None => return Waited::Finished,
            Some(t) => {
                if let Some(prompt) = &t.conflict_prompt {
                    return Waited::ConflictPrompt(prompt.file_name.clone());
                }
            }
        }
        assert!(Instant::now() < deadline, "transfer did not finish in time");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_finished(app: &mut AppState) {
    assert_eq!(wait(app), Waited::Finished);
}

fn enter_dir(app: &mut AppState, name: &str) {
    focus(app, name);
    app.handle_key_event(key(KeyCode::Enter));
    assert_eq!(app.current_dir.file_name().unwrap(), name);
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

/// `root/src/{a.txt, b.txt}` and `root/dst/{a.txt, b.txt}` with different contents.
fn conflicting_tree(root: &Path) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("dst")).unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(root.join("src").join(name), format!("new {name}")).unwrap();
        fs::write(root.join("dst").join(name), format!("old {name}")).unwrap();
    }
}

/// Selects every file in `root/src`, yanks (`y`) or cuts (`x`), then pastes into `root/dst`.
fn paste_all_from_src(app: &mut AppState, op: char) {
    enter_dir(app, "src");
    app.handle_key_event(common::ctrl('a'));
    app.handle_key_event(key(KeyCode::Char(op)));
    app.handle_key_event(key(KeyCode::Char('-')));
    enter_dir(app, "dst");
    app.handle_key_event(key(KeyCode::Char('p')));
}

// --- No conflict ---------------------------------------------------------------------------

#[test]
fn yank_and_paste_copies_file_and_keeps_clipboard() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("file.txt"), "hello").unwrap();
    fs::create_dir(root.join("dst")).unwrap();

    let mut app = fresh_app(root);
    focus(&mut app, "file.txt");
    app.handle_key_event(key(KeyCode::Char('y')));
    enter_dir(&mut app, "dst");
    app.handle_key_event(key(KeyCode::Char('p')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/file.txt")), "hello");
    assert_eq!(read(&root.join("file.txt")), "hello");
    assert_eq!(
        app.status_message,
        "Transfer complete: 1 succeeded / 0 failed"
    );
    assert!(app.clipboard.is_some(), "a copy can be pasted again");
    assert!(
        app.entries.iter().any(|e| e.name == "file.txt"),
        "the destination is reloaded"
    );
}

#[test]
fn yank_and_paste_copies_folder_recursively() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tree/sub")).unwrap();
    fs::write(root.join("tree/top.txt"), "top").unwrap();
    fs::write(root.join("tree/sub/deep.txt"), "deep").unwrap();
    fs::create_dir(root.join("dst")).unwrap();

    let mut app = fresh_app(root);
    focus(&mut app, "tree");
    app.handle_key_event(key(KeyCode::Char('y')));
    enter_dir(&mut app, "dst");
    app.handle_key_event(key(KeyCode::Char('p')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/tree/top.txt")), "top");
    assert_eq!(read(&root.join("dst/tree/sub/deep.txt")), "deep");
    assert!(root.join("tree/sub/deep.txt").is_file());
}

#[test]
fn cut_and_paste_moves_files_and_clears_both_clipboards() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("dst")).unwrap();
    fs::write(root.join("src/a.txt"), "a").unwrap();
    fs::write(root.join("src/b.txt"), "b").unwrap();

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'x');
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/a.txt")), "a");
    assert_eq!(read(&root.join("dst/b.txt")), "b");
    assert!(!root.join("src/a.txt").exists());
    assert!(!root.join("src/b.txt").exists());
    assert_eq!(
        app.status_message,
        "Transfer complete: 2 succeeded / 0 failed"
    );
    assert!(app.clipboard.is_none());
    assert!(tuif::fs_ops::os_clipboard::get_os_clipboard_files().is_none());
}

#[test]
fn empty_clipboard_reports_and_does_nothing() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    app.handle_key_event(key(KeyCode::Char('p')));
    assert!(app.active_transfer.is_none());
    assert_eq!(app.status_message, "Clipboard is empty");
}

// --- Conflict prompt -----------------------------------------------------------------------

#[test]
fn conflict_overwrite_replaces_one_file_then_asks_again() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'y');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    app.handle_key_event(key(KeyCode::Char('o')));
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("b.txt".into()));
    app.handle_key_event(key(KeyCode::Char('s')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/a.txt")), "new a.txt");
    assert_eq!(read(&root.join("dst/b.txt")), "old b.txt");
    assert_eq!(
        app.status_message,
        "Transfer complete: 1 succeeded / 0 failed"
    );
}

#[test]
fn conflict_overwrite_all_asks_only_once() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'y');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    app.handle_key_event(key(KeyCode::Char('a')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/a.txt")), "new a.txt");
    assert_eq!(read(&root.join("dst/b.txt")), "new b.txt");
    assert_eq!(
        app.status_message,
        "Transfer complete: 2 succeeded / 0 failed"
    );
}

#[test]
fn conflict_skip_all_asks_only_once_and_leaves_destination() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'y');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    app.handle_key_event(key(KeyCode::Char('k')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/a.txt")), "old a.txt");
    assert_eq!(read(&root.join("dst/b.txt")), "old b.txt");
    assert_eq!(
        app.status_message,
        "Transfer complete: 0 succeeded / 0 failed"
    );
}

#[test]
fn conflict_cursor_and_enter_choose_the_highlighted_button() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'y');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    // Buttons: Overwrite, Skip, Overwrite All, Skip All, Cancel. Left wraps from 0 to Cancel.
    app.handle_key_event(key(KeyCode::Left));
    app.handle_key_event(key(KeyCode::Right));
    app.handle_key_event(key(KeyCode::Right));
    app.handle_key_event(key(KeyCode::Tab));
    let cursor = app
        .active_transfer
        .as_ref()
        .and_then(|t| t.conflict_prompt.as_ref())
        .map(|p| p.cursor);
    assert_eq!(cursor, Some(2));
    app.handle_key_event(key(KeyCode::Enter));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/a.txt")), "new a.txt");
    assert_eq!(read(&root.join("dst/b.txt")), "new b.txt");
}

#[test]
fn conflict_cancel_stops_copy_and_keeps_destination() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'y');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    app.handle_key_event(key(KeyCode::Esc));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("dst/a.txt")), "old a.txt");
    assert_eq!(read(&root.join("dst/b.txt")), "old b.txt");
    assert_eq!(app.status_message, "Transfer cancelled (0 completed)");
}

#[test]
fn conflict_cancel_during_move_keeps_sources_and_clipboard() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'x');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    app.handle_key_event(key(KeyCode::Char('c')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("src/a.txt")), "new a.txt");
    assert_eq!(read(&root.join("src/b.txt")), "new b.txt");
    assert_eq!(read(&root.join("dst/a.txt")), "old a.txt");
    assert!(app.clipboard.is_some(), "a cancelled move can be retried");
}

/// Current behavior: a move whose conflicts were all skipped still counts as "no failures",
/// so the clipboard is cleared even though the skipped sources stay where they were.
#[test]
fn conflict_skip_during_move_keeps_source_and_clears_clipboard() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    conflicting_tree(root);

    let mut app = fresh_app(root);
    paste_all_from_src(&mut app, 'x');
    assert_eq!(wait(&mut app), Waited::ConflictPrompt("a.txt".into()));
    app.handle_key_event(key(KeyCode::Char('k')));
    wait_finished(&mut app);

    assert_eq!(read(&root.join("src/a.txt")), "new a.txt");
    assert_eq!(read(&root.join("dst/a.txt")), "old a.txt");
    assert!(app.clipboard.is_none());
}

// --- Pastes that must not touch data -------------------------------------------------------

#[test]
fn paste_into_same_folder_is_skipped_without_prompt() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("file.txt"), "keep").unwrap();

    for op in ['y', 'x'] {
        let mut app = fresh_app(root);
        focus(&mut app, "file.txt");
        app.handle_key_event(key(KeyCode::Char(op)));
        app.handle_key_event(key(KeyCode::Char('p')));
        wait_finished(&mut app);

        assert_eq!(read(&root.join("file.txt")), "keep", "op {op}");
        assert_eq!(
            app.status_message, "Transfer complete: 0 succeeded / 0 failed",
            "op {op}"
        );
    }
}

#[test]
fn paste_folder_into_itself_fails_and_keeps_source() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("box")).unwrap();
    fs::write(root.join("box/inner.txt"), "inner").unwrap();

    for op in ['y', 'x'] {
        let mut app = fresh_app(root);
        focus(&mut app, "box");
        app.handle_key_event(key(KeyCode::Char(op)));
        enter_dir(&mut app, "box");
        app.handle_key_event(key(KeyCode::Char('p')));
        wait_finished(&mut app);

        assert!(!root.join("box/box").exists(), "op {op}");
        assert_eq!(read(&root.join("box/inner.txt")), "inner", "op {op}");
        assert_eq!(
            app.status_message, "Transfer complete: 0 succeeded / 1 failed",
            "op {op}"
        );
        if op == 'x' {
            assert!(app.clipboard.is_some(), "a failed move keeps the clipboard");
        }
    }
}

// --- Remote guards (no SFTP session) --------------------------------------------------------

#[test]
fn remote_item_without_session_is_refused() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    app.clipboard = Some(FileClipboard {
        mode: ClipMode::Copy,
        items: vec![ClipboardItem::Remote {
            profile_id: "user_host".into(),
            path: "/home/user/file.txt".into(),
        }],
    });
    app.handle_key_event(key(KeyCode::Char('p')));

    assert!(app.active_transfer.is_none());
    assert_eq!(
        app.status_message,
        "Active remote session does not match download items"
    );
}

#[test]
fn remote_destination_without_session_is_refused() {
    let _clip = lock_clipboard();
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("file.txt"), "x").unwrap();

    let mut app = fresh_app(root);
    focus(&mut app, "file.txt");
    app.handle_key_event(key(KeyCode::Char('y')));
    app.tree_nodes = vec![TreeNode {
        path: root.to_path_buf(),
        name: "remote".into(),
        depth: 0,
        expanded: false,
        remote: Some(RemoteNodeInfo::Directory {
            profile_id: "user_host".into(),
            path: "/home/user".into(),
        }),
    }];
    app.tree_cursor = 0;
    app.focus_pane = FocusPane::Tree;
    app.handle_key_event(key(KeyCode::Char('p')));

    assert!(app.active_transfer.is_none());
    assert_eq!(
        app.status_message,
        "Remote session is not active for destination"
    );
}
