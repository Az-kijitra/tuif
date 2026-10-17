//! Integration tests for `cuifiler` (Vim-like keybinding navigation, file operations, search,
//! viewer overlay, external CUI tool resolution, and headless Ratatui rendering).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use cuifiler::app::{AppState, FocusPane, OverlayMode};
use cuifiler::tui;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn vim_navigation_and_file_ops_workflow() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("dir_a")).unwrap();
    std::fs::write(root.join("file1.txt"), b"hello cuifiler\nline 2").unwrap();
    std::fs::write(root.join("file2.md"), b"# Title\nmarkdown body").unwrap();

    let mut app = AppState::new(root.to_path_buf());
    app.ui_state = cuifiler::shared::schema::ui_state::UiStateData::default();
    app.reload_dir();
    assert_eq!(app.entries.len(), 3);
    // Directories come first (`dir_a`, `file1.txt`, `file2.md`)
    assert_eq!(app.focused_entry().unwrap().name, "dir_a");

    // `j` moves down to `file1.txt`
    app.handle_key_event(key(KeyCode::Char('j')));
    assert_eq!(app.focused_entry().unwrap().name, "file1.txt");

    // `G` (Shift+G) moves to bottom (`file2.md`)
    app.handle_key_event(key_mod(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(app.focused_entry().unwrap().name, "file2.md");

    // `gg` moves back to top (`dir_a`)
    app.handle_key_event(key(KeyCode::Char('g')));
    app.handle_key_event(key(KeyCode::Char('g')));
    assert_eq!(app.focused_entry().unwrap().name, "dir_a");

    // `a` opens NewFile prompt -> type `created.txt` -> Enter
    app.handle_key_event(key(KeyCode::Char('a')));
    for ch in "created.txt".chars() {
        app.handle_key_event(key(KeyCode::Char(ch)));
    }
    app.handle_key_event(key(KeyCode::Enter));
    assert!(root.join("created.txt").is_file());

    // Move to `file1.txt`, yank (`y`), enter `dir_a`, paste (`p`)
    while app.focused_entry().unwrap().name != "file1.txt" {
        app.handle_key_event(key(KeyCode::Char('j')));
    }
    app.handle_key_event(key(KeyCode::Char('y')));
    assert!(app.clipboard.is_some());

    // `gg` to top (`created.txt`), then `j` to `dir_a`, `Enter` to enter `dir_a`
    app.handle_key_event(key(KeyCode::Char('g')));
    app.handle_key_event(key(KeyCode::Char('g')));
    while app.focused_entry().unwrap().name != "dir_a" {
        app.handle_key_event(key(KeyCode::Char('j')));
    }
    app.handle_key_event(key(KeyCode::Enter));
    assert_eq!(app.current_dir, root.join("dir_a"));

    // `p` to paste `file1.txt` inside `dir_a`
    app.handle_key_event(key(KeyCode::Char('p')));
    assert!(root.join("dir_a").join("file1.txt").is_file());

    // `Enter` on `file1.txt` opens the built-in TUI text viewer
    app.handle_key_event(key(KeyCode::Enter));
    assert!(matches!(app.overlay, OverlayMode::Viewer { .. }));

    // Render viewer in TestBackend
    let backend = TestBackend::new(90, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| tui::draw(f, &app)).unwrap();

    // `q` closes viewer
    app.handle_key_event(key(KeyCode::Char('q')));
    assert_eq!(app.overlay, OverlayMode::None);

    // `-` (Vim-style parent dir) returns to `root`
    app.handle_key_event(key(KeyCode::Char('-')));
    assert_eq!(app.current_dir, root);

    // `/` incremental search filters to `file2`
    app.handle_key_event(key(KeyCode::Char('/')));
    for ch in "file2".chars() {
        app.handle_key_event(key(KeyCode::Char(ch)));
    }
    app.handle_key_event(key(KeyCode::Enter));
    assert_eq!(app.visible_indices.len(), 1);
    assert_eq!(app.focused_entry().unwrap().name, "file2.md");

    // `Esc` clears search filter
    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.visible_indices.len(), 4);

    // `Tab` switches focus between FileList and Tree
    assert_eq!(app.focus_pane, FocusPane::FileList);
    app.handle_key_event(key(KeyCode::Tab));
    assert_eq!(app.focus_pane, FocusPane::Tree);

    // Tree automatically expanded ancestors down to `root` and shows child folder `dir_a`
    assert!(app.tree_nodes.iter().any(|n| n.name == "dir_a"));

    // Move cursor to `dir_a` in tree and press Enter to open it in FileList
    if let Some(idx) = app.tree_nodes.iter().position(|n| n.name == "dir_a") {
        app.tree_cursor = idx;
        // `l` expands `dir_a`, `h` collapses `dir_a`
        app.handle_key_event(key(KeyCode::Char('l')));
        assert!(app.tree_nodes[idx].expanded);
        app.handle_key_event(key(KeyCode::Char('h')));
        assert!(!app.tree_nodes[idx].expanded);

        // `Enter` opens `dir_a` in the FileList and returns focus to FileList
        app.handle_key_event(key(KeyCode::Enter));
        assert_eq!(app.focus_pane, FocusPane::FileList);
        assert_eq!(app.current_dir, root.join("dir_a"));
    }
}

#[test]
fn multi_column_left_right_navigation_tracks_terminal_height() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for i in 0..15 {
        std::fs::write(root.join(format!("item_{i:02}.txt")), b"x").unwrap();
    }

    let mut app = AppState::new(root.to_path_buf());
    app.ui_state = cuifiler::shared::schema::ui_state::UiStateData::default();
    app.reload_dir();

    // Terminal height 11 -> top_rows(2) + status(1) + borders(2) = 5 -> list_rows_per_col = 6
    app.update_viewport_size(120, 11);
    assert_eq!(app.list_rows_per_col, 6);

    // Move down to row 2 of Column 0 (vis_idx = 2)
    app.handle_key_event(key(KeyCode::Char('j')));
    app.handle_key_event(key(KeyCode::Char('j')));
    assert_eq!(app.focused_vis_idx, 2);

    // `l` (Right) moves to row 2 of Column 1 (vis_idx = 2 + 6 = 8)
    app.handle_key_event(key(KeyCode::Char('l')));
    assert_eq!(app.focused_vis_idx, 8);

    // `l` (Right) moves to Column 2 (vis_idx = 8 + 6 = 14)
    app.handle_key_event(key(KeyCode::Char('l')));
    assert_eq!(app.focused_vis_idx, 14);

    // Already in rightmost column (Column 2) -> `l` does not move
    app.handle_key_event(key(KeyCode::Char('l')));
    assert_eq!(app.focused_vis_idx, 14);

    // `h` (Left) moves back to Column 1 (vis_idx = 14 - 6 = 8)
    app.handle_key_event(key(KeyCode::Char('h')));
    assert_eq!(app.focused_vis_idx, 8);

    // `h` (Left) moves back to Column 0 (vis_idx = 8 - 6 = 2)
    app.handle_key_event(key(KeyCode::Char('h')));
    assert_eq!(app.focused_vis_idx, 2);
}

#[test]
fn parent_navigation_preserves_cursor_on_child_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("alpha_dir")).unwrap();
    std::fs::create_dir_all(root.join("beta_dir").join("nested")).unwrap();
    std::fs::create_dir_all(root.join("gamma_dir")).unwrap();

    // Start inside `beta_dir/nested`
    let mut app = AppState::new(root.join("beta_dir").join("nested"));
    app.ui_state = cuifiler::shared::schema::ui_state::UiStateData::default();
    app.reload_dir();

    // Go up 1 level (`-` -> `beta_dir`) -> cursor is on `nested`
    app.handle_key_event(key(KeyCode::Char('-')));
    assert_eq!(app.current_dir, root.join("beta_dir"));
    assert_eq!(app.focused_entry().unwrap().name, "nested");

    // Go up 1 more level (`Backspace` -> `root`) -> cursor is on `beta_dir` (index 1, not 0 `alpha_dir`)
    app.handle_key_event(key(KeyCode::Backspace));
    assert_eq!(app.current_dir, root);
    assert_eq!(app.focused_entry().unwrap().name, "beta_dir");
    assert_eq!(app.focused_vis_idx, 1);

    // Multi-level jump from `beta_dir/nested` directly to `root` also focuses `beta_dir`
    app.navigate_to(root.join("beta_dir").join("nested"));
    app.navigate_to(root.to_path_buf());
    assert_eq!(app.focused_entry().unwrap().name, "beta_dir");
}



