//! Characterization tests for `AppState::execute_action`: cursor movement, selection, the tree
//! pane, toggles, delete, and the actions that open a dialog or menu.
//!
//! They pin the current behavior before the function is split up. Paste is covered by
//! `paste_tests.rs`, and the dialogs' own key handling by `overlay_key_tests.rs`.

mod common;

use std::fs;
use std::path::Path;

use common::{focus, fresh_app};
use tuif::app::{normalize_tree_key, AppState, ClipMode, FocusPane, InputPromptKind, OverlayMode};
use tuif::keymap::ActionId;

/// `a.txt` .. `e.txt` (five files, no folders).
fn five_files(root: &Path) {
    for n in ["a", "b", "c", "d", "e"] {
        fs::write(root.join(format!("{n}.txt")), n).unwrap();
    }
}

fn focused_name(app: &AppState) -> String {
    app.focused_entry().unwrap().name.clone()
}

fn input(app: &AppState) -> (&InputPromptKind, &str, &str) {
    match &app.overlay {
        OverlayMode::Input {
            kind,
            title,
            buffer,
        } => (kind, title.as_str(), buffer.as_str()),
        other => panic!("expected an input prompt, got {other:?}"),
    }
}

/// Puts the tree cursor on the node for `path` and focuses the tree.
fn tree_cursor_on(app: &mut AppState, path: &Path) -> usize {
    let key = normalize_tree_key(path);
    let idx = app
        .tree_nodes
        .iter()
        .position(|n| n.remote.is_none() && normalize_tree_key(&n.path) == key)
        .expect("node for path in tree");
    app.focus_pane = FocusPane::Tree;
    app.tree_cursor = idx;
    idx
}

// --- Cursor in the file list ---------------------------------------------------------------

#[test]
fn list_cursor_moves_and_clamps() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = fresh_app(tmp.path());
    app.list_rows_per_col = 2;

    app.execute_action(ActionId::MoveUp);
    assert_eq!(app.focused_vis_idx, 0, "MoveUp stops at the top");
    app.execute_action(ActionId::MoveDown);
    assert_eq!(focused_name(&app), "b.txt");
    app.execute_action(ActionId::MoveBottom);
    assert_eq!(focused_name(&app), "e.txt");
    app.execute_action(ActionId::MoveDown);
    assert_eq!(focused_name(&app), "e.txt", "MoveDown stops at the bottom");
    app.execute_action(ActionId::PageUp);
    assert_eq!(app.focused_vis_idx, 2, "PageUp moves by one column");
    app.execute_action(ActionId::PageUp);
    app.execute_action(ActionId::PageUp);
    assert_eq!(app.focused_vis_idx, 0);
    app.execute_action(ActionId::PageDown);
    assert_eq!(app.focused_vis_idx, 2);
    app.execute_action(ActionId::PageDown);
    app.execute_action(ActionId::PageDown);
    assert_eq!(app.focused_vis_idx, 4, "PageDown clamps to the last entry");
    app.execute_action(ActionId::MoveTop);
    assert_eq!(app.focused_vis_idx, 0);
}

#[test]
fn list_left_right_move_between_columns() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = fresh_app(tmp.path());
    app.list_rows_per_col = 2; // columns: [a b] [c d] [e]

    app.execute_action(ActionId::MoveLeft);
    assert_eq!(app.focused_vis_idx, 0, "no column to the left");
    app.execute_action(ActionId::MoveRight);
    app.execute_action(ActionId::MoveRight);
    assert_eq!(focused_name(&app), "e.txt");
    app.execute_action(ActionId::MoveRight);
    assert_eq!(focused_name(&app), "e.txt", "no column to the right");
    app.execute_action(ActionId::MoveLeft);
    assert_eq!(focused_name(&app), "c.txt");

    app.focused_vis_idx = 3; // d.txt, the last column is shorter
    app.execute_action(ActionId::MoveRight);
    assert_eq!(
        focused_name(&app),
        "e.txt",
        "clamps into the shorter column"
    );
}

#[test]
fn moves_on_an_empty_folder_do_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    for action in [
        ActionId::MoveDown,
        ActionId::MoveRight,
        ActionId::MoveBottom,
        ActionId::PageDown,
        ActionId::SelectToggle,
    ] {
        app.execute_action(action);
        assert_eq!(app.focused_vis_idx, 0, "{action:?}");
    }
    assert!(app.selected_paths.is_empty());
}

// --- Selection -----------------------------------------------------------------------------

#[test]
fn select_toggle_all_and_clear() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = fresh_app(tmp.path());

    app.execute_action(ActionId::SelectToggle);
    assert!(app.selected_paths.contains(&tmp.path().join("a.txt")));
    app.execute_action(ActionId::SelectToggle);
    assert!(app.selected_paths.is_empty(), "toggling again deselects");

    // SelectAll only selects what the search filter shows.
    app.search_query = "b".into();
    app.apply_filter();
    app.execute_action(ActionId::SelectAll);
    assert_eq!(app.selected_paths.len(), 1);

    app.status_message = "something".into();
    app.execute_action(ActionId::ClearSelection);
    assert!(app.selected_paths.is_empty());
    assert!(
        app.search_query.is_empty(),
        "ClearSelection also clears the search"
    );
    assert_eq!(app.visible_indices.len(), 5);
    assert!(app.status_message.is_empty());
}

// --- Tree pane -----------------------------------------------------------------------------

#[test]
fn tree_left_collapses_then_goes_to_parent_and_right_expands_then_enters() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("child")).unwrap();
    let mut app = fresh_app(root);
    app.rebuild_tree();

    let idx = tree_cursor_on(&mut app, root);
    let depth = app.tree_nodes[idx].depth;
    assert!(
        app.tree_nodes[idx].expanded,
        "current folder starts expanded"
    );

    app.execute_action(ActionId::MoveLeft);
    assert_eq!(app.tree_cursor, idx);
    assert!(!app.tree_nodes[idx].expanded, "first Left collapses");

    app.execute_action(ActionId::MoveLeft);
    assert!(app.tree_cursor < idx, "second Left goes to the parent");
    assert_eq!(app.tree_nodes[app.tree_cursor].depth, depth - 1);

    let idx = tree_cursor_on(&mut app, root);
    app.execute_action(ActionId::MoveRight);
    assert!(app.tree_nodes[idx].expanded, "first Right expands");
    app.execute_action(ActionId::MoveRight);
    assert_eq!(
        app.tree_cursor,
        idx + 1,
        "second Right enters the first child"
    );
    assert_eq!(app.tree_nodes[idx + 1].name, "child");
}

#[test]
fn tree_select_toggle_flips_expansion_and_cursor_moves_clamp() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("child")).unwrap();
    let mut app = fresh_app(root);
    app.rebuild_tree();

    let idx = tree_cursor_on(&mut app, root);
    app.execute_action(ActionId::SelectToggle);
    assert!(!app.tree_nodes[idx].expanded);
    app.execute_action(ActionId::SelectToggle);
    assert!(app.tree_nodes[idx].expanded);
    assert!(
        app.selected_paths.is_empty(),
        "tree toggling never selects files"
    );

    app.execute_action(ActionId::MoveDown);
    assert_eq!(app.tree_cursor, idx + 1);
    app.execute_action(ActionId::MoveUp);
    assert_eq!(app.tree_cursor, idx);
    app.execute_action(ActionId::MoveBottom);
    assert_eq!(app.tree_cursor, app.tree_nodes.len() - 1);
    app.execute_action(ActionId::PageDown);
    assert_eq!(app.tree_cursor, app.tree_nodes.len() - 1);
    app.execute_action(ActionId::MoveTop);
    assert_eq!(app.tree_cursor, 0);
    app.execute_action(ActionId::PageUp);
    assert_eq!(app.tree_cursor, 0);
    assert_eq!(app.focused_vis_idx, 0, "the file list cursor is untouched");
}

#[test]
fn tree_open_navigates_and_focuses_the_file_list() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("child")).unwrap();
    let mut app = fresh_app(root);
    app.rebuild_tree();

    tree_cursor_on(&mut app, &root.join("child"));
    app.execute_action(ActionId::Open);
    assert_eq!(app.focus_pane, FocusPane::FileList);
    assert_eq!(
        normalize_tree_key(&app.current_dir),
        normalize_tree_key(&root.join("child"))
    );
}

// --- Open / parent / path bar --------------------------------------------------------------

#[test]
fn open_enters_folders_and_views_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("note.txt"), "hello").unwrap();
    let mut app = fresh_app(root);

    focus(&mut app, "note.txt");
    app.execute_action(ActionId::Open);
    assert!(matches!(app.overlay, OverlayMode::Viewer { .. }));
    app.overlay = OverlayMode::None;

    focus(&mut app, "sub");
    app.execute_action(ActionId::Open);
    assert_eq!(app.current_dir, root.join("sub"));

    app.execute_action(ActionId::GoParent);
    assert_eq!(app.current_dir, root);
}

#[test]
fn go_parent_at_a_drive_root_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    let drive_root = tmp.path().ancestors().last().unwrap().to_path_buf();
    app.navigate_to(drive_root.clone());
    app.execute_action(ActionId::GoParent);
    assert_eq!(app.current_dir, drive_root);
}

#[test]
fn focus_path_prefills_the_current_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    app.execute_action(ActionId::FocusPath);
    let (kind, title, buffer) = input(&app);
    assert_eq!(kind, &InputPromptKind::PathBar);
    assert_eq!(title, "Enter Path (Enter:Go / Esc:Cancel)");
    assert_eq!(buffer, app.active_display_path());
}

// --- Toggles -------------------------------------------------------------------------------

#[test]
fn toggles_flip_ui_state_and_report() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());

    let hidden = app.ui_state.ui_show_hidden;
    app.execute_action(ActionId::ToggleHidden);
    assert_eq!(app.ui_state.ui_show_hidden, !hidden);
    let expected = if hidden { "OFF" } else { "ON" };
    assert_eq!(app.status_message, format!("Hidden files: {expected}"));

    let size = app.ui_state.ui_show_size;
    app.execute_action(ActionId::ToggleSize);
    assert_eq!(app.ui_state.ui_show_size, !size);

    let time = app.ui_state.ui_show_time;
    app.execute_action(ActionId::ToggleTime);
    assert_eq!(app.ui_state.ui_show_time, !time);

    app.ui_state.ui_show_tree = true;
    app.focus_pane = FocusPane::Tree;
    app.execute_action(ActionId::ToggleTree);
    assert!(!app.ui_state.ui_show_tree);
    assert_eq!(
        app.focus_pane,
        FocusPane::FileList,
        "hiding the tree moves focus to the file list"
    );
    app.execute_action(ActionId::ToggleTree);
    assert!(app.ui_state.ui_show_tree);
    assert_eq!(app.focus_pane, FocusPane::FileList);
}

// --- Copy / cut ----------------------------------------------------------------------------

#[cfg(windows)]
#[test]
fn copy_and_cut_fill_the_clipboard_and_report() {
    use tuif::fs_ops::os_clipboard::clear_os_clipboard;

    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = fresh_app(tmp.path());

    app.execute_action(ActionId::SelectAll);
    app.execute_action(ActionId::Copy);
    let clip = app.clipboard.as_ref().unwrap();
    assert_eq!(clip.mode, ClipMode::Copy);
    assert_eq!(clip.items.len(), 5);
    assert_eq!(app.status_message, "Yanked 5 item(s) (press p to paste)");

    app.execute_action(ActionId::ClearSelection);
    app.execute_action(ActionId::Cut);
    let clip = app.clipboard.as_ref().unwrap();
    assert_eq!(clip.mode, ClipMode::Cut);
    assert_eq!(
        clip.items.len(),
        1,
        "without a selection, the focused entry"
    );
    assert_eq!(app.status_message, "Cut 1 item(s) (press p to move)");

    clear_os_clipboard();
}

// --- Delete --------------------------------------------------------------------------------

#[test]
fn delete_asks_with_a_title_that_matches_trash_or_permanent() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = fresh_app(tmp.path());
    app.general.confirm_delete = true;
    app.general.delete_to_trash = true;
    app.execute_action(ActionId::SelectAll);

    app.execute_action(ActionId::Delete);
    let (kind, title, _) = input(&app);
    assert_eq!(
        title,
        "Move 5 item(s) to Trash? (y/Enter:Confirm / Esc:Cancel)"
    );
    assert!(matches!(
        kind,
        InputPromptKind::ConfirmDelete { permanent: false, targets } if targets.len() == 5
    ));

    app.overlay = OverlayMode::None;
    app.execute_action(ActionId::DeletePermanent);
    let (kind, title, _) = input(&app);
    assert_eq!(
        title,
        "Permanently delete 5 item(s)? (y/Enter:Confirm / Esc:Cancel)"
    );
    assert!(matches!(
        kind,
        InputPromptKind::ConfirmDelete {
            permanent: true,
            ..
        }
    ));

    // With trash disabled, plain Delete is permanent too.
    app.overlay = OverlayMode::None;
    app.general.delete_to_trash = false;
    app.execute_action(ActionId::Delete);
    assert!(matches!(
        input(&app).0,
        InputPromptKind::ConfirmDelete {
            permanent: true,
            ..
        }
    ));
    assert_eq!(
        fs::read_dir(tmp.path()).unwrap().count(),
        5,
        "nothing deleted yet"
    );
}

#[test]
fn delete_without_confirmation_deletes_and_clears_selection() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = fresh_app(tmp.path());
    app.general.confirm_delete = false;

    focus(&mut app, "b.txt");
    app.execute_action(ActionId::SelectToggle);
    focus(&mut app, "c.txt");
    app.execute_action(ActionId::SelectToggle);
    app.execute_action(ActionId::DeletePermanent);

    assert_eq!(app.overlay, OverlayMode::None);
    assert!(!tmp.path().join("b.txt").exists());
    assert!(!tmp.path().join("c.txt").exists());
    assert!(tmp.path().join("a.txt").exists());
    assert!(app.selected_paths.is_empty());
    assert_eq!(
        app.status_message,
        "Delete complete: 2 succeeded / 0 failed"
    );
    assert_eq!(app.visible_indices.len(), 3, "the list is reloaded");
}

#[test]
fn delete_in_an_empty_folder_does_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    app.general.confirm_delete = true;
    app.execute_action(ActionId::Delete);
    assert_eq!(app.overlay, OverlayMode::None);
}

// --- Dialogs and menus ---------------------------------------------------------------------

#[test]
fn rename_prefills_the_name_in_the_list_and_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("child")).unwrap();
    fs::write(root.join("file.txt"), "x").unwrap();
    let mut app = fresh_app(root);

    focus(&mut app, "file.txt");
    app.execute_action(ActionId::Rename);
    let (kind, title, buffer) = input(&app);
    assert_eq!(
        kind,
        &InputPromptKind::Rename {
            target: root.join("file.txt")
        }
    );
    assert_eq!(title, "Rename (Enter:Apply / Esc:Cancel)");
    assert_eq!(buffer, "file.txt");

    app.overlay = OverlayMode::None;
    app.rebuild_tree();
    let idx = tree_cursor_on(&mut app, &root.join("child"));
    let node_path = app.tree_nodes[idx].path.clone();
    app.execute_action(ActionId::Rename);
    let (kind, _, buffer) = input(&app);
    assert_eq!(kind, &InputPromptKind::Rename { target: node_path });
    assert_eq!(buffer, "child");
}

#[test]
fn prompt_actions_open_empty_or_prefilled_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());

    let cases = [
        (
            ActionId::NewFile,
            InputPromptKind::NewFile,
            "New File Name (Enter:Create / Esc:Cancel)",
        ),
        (
            ActionId::NewFolder,
            InputPromptKind::NewFolder,
            "New Folder Name (Enter:Create / Esc:Cancel)",
        ),
    ];
    for (action, expected_kind, expected_title) in cases {
        app.overlay = OverlayMode::None;
        app.execute_action(action);
        let (kind, title, buffer) = input(&app);
        assert_eq!(kind, &expected_kind);
        assert_eq!(title, expected_title);
        assert_eq!(buffer, "");
    }

    app.overlay = OverlayMode::None;
    app.search_query = "abc".into();
    app.execute_action(ActionId::Search);
    let (kind, title, buffer) = input(&app);
    assert_eq!(kind, &InputPromptKind::Search);
    assert_eq!(
        title,
        "Search Filter (Ctrl+R:Toggle Regex / Enter:Keep / Esc:Clear)"
    );
    assert_eq!(buffer, "abc", "search starts from the current query");
}

#[test]
fn menu_actions_open_their_overlays_at_the_top() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    let cases = [
        (ActionId::SortMenu, OverlayMode::SortMenu { cursor: 0 }),
        (ActionId::ActionMenu, OverlayMode::ActionMenu { cursor: 0 }),
        (
            ActionId::ConnectionManager,
            OverlayMode::ConnectionManager { cursor: 0 },
        ),
    ];
    for (action, expected) in cases {
        app.overlay = OverlayMode::None;
        app.execute_action(action);
        assert_eq!(app.overlay, expected, "{action:?}");
    }
}

#[test]
fn history_and_jump_list_popups_show_their_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    app.nav_data.visit_history = vec!["C:\\one".into(), "C:\\two".into()];
    app.nav_data.jump_list = vec!["C:\\jump".into()];

    app.execute_action(ActionId::History);
    match &app.overlay {
        OverlayMode::ListPopup {
            title,
            items,
            cursor,
            is_jump_list,
        } => {
            assert_eq!(
                title,
                "Visit History (j/k:Select / Enter:Go / d:Remove / Esc:Close)"
            );
            assert_eq!(items, &app.nav_data.visit_history);
            assert_eq!(*cursor, 0);
            assert!(!is_jump_list);
        }
        other => panic!("unexpected overlay {other:?}"),
    }

    app.overlay = OverlayMode::None;
    app.execute_action(ActionId::HistoryJumpList);
    match &app.overlay {
        OverlayMode::ListPopup {
            title,
            items,
            is_jump_list,
            ..
        } => {
            assert_eq!(
                title,
                "Jump List (j/k:Select / Enter:Go / d:Remove / Esc:Close)"
            );
            assert_eq!(items, &vec!["C:\\jump".to_string()]);
            assert!(is_jump_list);
        }
        other => panic!("unexpected overlay {other:?}"),
    }
}

#[test]
fn jump_add_puts_the_folder_first_without_duplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    let here = tmp.path().to_string_lossy().into_owned();
    app.nav_data.jump_list = vec!["C:\\other".into(), here.to_ascii_uppercase()];

    app.execute_action(ActionId::JumpAdd);
    assert_eq!(
        app.nav_data.jump_list,
        vec![here.clone(), "C:\\other".to_string()],
        "the existing entry is replaced regardless of letter case"
    );
    assert_eq!(app.status_message, format!("Added to Jump List: {here}"));
}

#[test]
fn zip_create_suggests_a_name_from_the_targets() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = fresh_app(tmp.path());
    app.execute_action(ActionId::ZipCreate);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.status_message, "No items selected to compress");

    five_files(tmp.path());
    app.reload_dir();
    focus(&mut app, "c.txt");
    app.execute_action(ActionId::ZipCreate);
    let (kind, title, buffer) = input(&app);
    assert_eq!(kind, &InputPromptKind::ZipCreate);
    assert_eq!(title, "Output ZIP File Name (Enter:Create / Esc:Cancel)");
    assert_eq!(buffer, "c.zip");

    app.overlay = OverlayMode::None;
    app.execute_action(ActionId::SelectAll);
    app.execute_action(ActionId::ZipCreate);
    assert_eq!(input(&app).2, "archive.zip");
}

#[test]
fn zip_extract_requires_a_zip_file_in_the_list() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("plain.txt"), "x").unwrap();
    fs::write(root.join("Data.ZIP"), "not really a zip").unwrap();
    let mut app = fresh_app(root);

    focus(&mut app, "plain.txt");
    app.execute_action(ActionId::ZipExtract);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.status_message, "Please select a .zip file");

    focus(&mut app, "Data.ZIP");
    app.focus_pane = FocusPane::Tree;
    app.execute_action(ActionId::ZipExtract);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.status_message, "Please select a .zip file in Files");

    app.focus_pane = FocusPane::FileList;
    app.execute_action(ActionId::ZipExtract);
    let (kind, title, buffer) = input(&app);
    assert_eq!(
        kind,
        &InputPromptKind::ZipExtract {
            zip_path: root.join("Data.ZIP")
        }
    );
    assert_eq!(
        title,
        "Extract Destination Folder (Enter:Extract / Esc:Cancel)"
    );
    assert_eq!(buffer, "Data", "the extension check ignores letter case");
}
