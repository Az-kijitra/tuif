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
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        app.poll_active_transfer();
        if app.active_transfer.is_none() {
            break;
        }
    }
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

#[test]
fn mouse_explorer_workflow_left_double_and_right_click_action_menu() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("sub_folder")).unwrap();
    std::fs::write(root.join("alpha.txt"), b"alpha").unwrap();
    std::fs::write(root.join("beta.txt"), b"beta").unwrap();

    let mut app = AppState::new(root.to_path_buf());
    app.ui_state = cuifiler::shared::schema::ui_state::UiStateData::default();
    app.reload_dir();
    app.update_viewport_size(100, 28);

    // Entries sorted by name asc: 0=`alpha.txt` (row 3), 1=`beta.txt` (row 4), 2=`sub_folder` (row 5)
    // Single left-click on row 4 (`beta.txt`) focuses `beta.txt`
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 30,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.focused_entry().unwrap().name, "beta.txt");

    // Right-click on row 3 (`alpha.txt`) focuses `alpha.txt` and opens ActionMenu
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: 30,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.focused_entry().unwrap().name, "alpha.txt");
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });

    // Click row 1 of ActionMenu ("Copy / Yank (y)") -> yanks `alpha.txt` and closes ActionMenu
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 28,
    };
    let (popup, _, _) = tui::popup_list_geometry(area, 10 + app.external_apps.len(), 0);
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: popup.x + 3,
        row: popup.y + 1 + 1, // item index 1 = Copy / Yank
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.overlay, OverlayMode::None);
    assert!(app.clipboard.is_some());

    // Double left-click on row 5 (`sub_folder`) opens `sub_folder`
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 30,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 30,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.current_dir, root.join("sub_folder"));
}

#[test]
fn tree_right_click_action_menu_and_gui_apps_workflow() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use cuifiler::app::{FocusPane, InputPromptKind};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let dir_a = root.join("alpha_dir");
    let dir_b = root.join("beta_dir");
    std::fs::create_dir(&dir_a).unwrap();
    std::fs::create_dir(&dir_b).unwrap();

    let mut app = AppState::new(root.to_path_buf());
    app.ui_state = cuifiler::shared::schema::ui_state::UiStateData::default();
    app.reload_dir();
    app.update_viewport_size(100, 28);

    // Verify GUI external apps are registered
    assert!(app.external_apps.iter().any(|a| a.name == "Open VSCode" && a.is_gui));
    assert!(app.external_apps.iter().any(|a| a.name == "Open Explorer" && a.is_gui));

    // Find `beta_dir` in `app.tree_nodes`
    let beta_tree_idx = app
        .tree_nodes
        .iter()
        .position(|n| n.name == "beta_dir")
        .expect("beta_dir should be visible in tree_nodes");

    let page_size = (28u16.saturating_sub(3).saturating_sub(2)).max(1) as usize;
    app.tree_cursor = (beta_tree_idx / page_size) * page_size;
    let row_in_tree = (beta_tree_idx % page_size) as u16 + 3; // tree_rect.y = 2, inner starts at 3

    // Right-click `beta_dir` in Drives / Tree pane (column 5 is inside tree_rect)
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: 5,
        row: row_in_tree,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.focus_pane, FocusPane::Tree);
    assert_eq!(app.tree_cursor, beta_tree_idx);
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });
    assert_eq!(app.operation_targets(), vec![dir_b.clone()]);

    // Select item 4 ("Rename (r)") in ActionMenu -> opens Rename prompt targeting `beta_dir`
    let area = ratatui::layout::Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 28,
    };
    let (popup, _, _) = tui::popup_list_geometry(area, 10 + app.external_apps.len(), 0);
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: popup.x + 3,
        row: popup.y + 1 + 4, // item index 4 = Rename
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.overlay,
        OverlayMode::Input {
            kind: InputPromptKind::Rename { target: dir_b },
            title: "Rename (Enter:Apply / Esc:Cancel)".to_string(),
            buffer: "beta_dir".to_string(),
        }
    );
}

#[test]
fn tree_selection_switches_from_remote_to_local_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let local_sub = root.join("local_folder");
    std::fs::create_dir(&local_sub).unwrap();

    let mut app = AppState::new(root.to_path_buf());
    app.ui_state.ui_show_tree = true;

    // Simulate an active remote SFTP state
    app.remote_dir = Some("/var/www/html".to_string());
    assert!(app.is_remote());

    // Switch focus to Tree pane and find the local folder node
    app.focus_pane = FocusPane::Tree;
    let local_node_idx = app
        .tree_nodes
        .iter()
        .position(|n| n.path == local_sub)
        .expect("local_sub must exist in tree");
    app.tree_cursor = local_node_idx;

    // Press Enter to open the local folder from Tree
    app.handle_key_event(key(KeyCode::Enter));

    // Must switch back to local mode
    assert!(!app.is_remote(), "remote_dir must be cleared to None");
    assert_eq!(app.remote_dir, None);
    assert_eq!(app.current_dir, local_sub);
    assert_eq!(app.focus_pane, FocusPane::FileList);
}

#[test]
fn fuzzy_search_shortcuts_and_post_selection_navigation() {
    use cuifiler::external_apps::{FuzzyKind, FuzzySelection};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sub_dir = root.join("src").join("nested");
    std::fs::create_dir_all(&sub_dir).unwrap();
    let file_a = sub_dir.join("alpha.rs");
    let file_b = sub_dir.join("beta_target.rs");
    let file_c = sub_dir.join("gamma.rs");
    std::fs::write(&file_a, "fn alpha() {}\n").unwrap();
    std::fs::write(&file_b, "fn beta() {}\n").unwrap();
    std::fs::write(&file_c, "fn gamma() {}\n").unwrap();

    let mut app = AppState::new(root.to_path_buf());

    // 1. Ctrl+P triggers FuzzyFindFile
    app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    let pending = app.pending_fuzzy.take().expect("Ctrl+P should set pending_fuzzy");
    assert_eq!(pending.kind, FuzzyKind::FindFile);
    assert_eq!(pending.working_dir, root.to_path_buf());

    // 2. Shift+F ('F') triggers FuzzyLiveGrep
    app.handle_key_event(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT));
    let pending = app.pending_fuzzy.take().expect("Shift+F should set pending_fuzzy");
    assert_eq!(pending.kind, FuzzyKind::LiveGrep);

    // 3. 'f' triggers FuzzyJumpDir
    app.handle_key_event(key(KeyCode::Char('f')));
    let pending = app.pending_fuzzy.take().expect("'f' should set pending_fuzzy");
    assert_eq!(pending.kind, FuzzyKind::JumpDir);

    // 4. Selecting a file in FuzzyFindFile / FuzzyLiveGrep navigates to its parent folder and focuses the file
    app.apply_fuzzy_selection(FuzzySelection {
        kind: FuzzyKind::LiveGrep,
        target_path: file_b.clone(),
        line_number: Some(12),
    });
    assert_eq!(app.current_dir, sub_dir);
    assert_eq!(app.focus_pane, FocusPane::FileList);
    let focused = app.focused_entry().expect("Should have a focused entry");
    assert_eq!(focused.path, file_b);
    assert!(app.status_message.contains("line 12"));

    // 5. Selecting a folder in FuzzyJumpDir navigates directly into that folder
    app.apply_fuzzy_selection(FuzzySelection {
        kind: FuzzyKind::JumpDir,
        target_path: root.join("src"),
        line_number: None,
    });
    assert_eq!(app.current_dir, root.join("src"));
    assert_eq!(app.focus_pane, FocusPane::FileList);
}


