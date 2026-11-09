//! Characterization tests for mouse handling: the main panes (`handle_mouse_event`) and every
//! overlay (`handle_overlay_mouse`).
//!
//! They pin the current behavior before the code is split up. Click positions come from the
//! same layout helpers the renderer uses (`tui::main_panes_geometry`, `popup_list_geometry`,
//! `menubar_dropdown_rect`).

mod common;

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};

use common::{focus, fresh_app};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use tuif::app::{
    normalize_tree_key, ActiveTransfer, AppState, FocusPane, InputPromptKind, OverlayMode,
    TransferProgressState, ACTION_MENU_FIXED_COUNT,
};
use tuif::keymap::ActionId;
use tuif::remote::profile::ConnectionProfile;
use tuif::tui;

const W: u16 = 100;
const H: u16 = 28;

fn area() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    }
}

fn app_in(root: &Path) -> AppState {
    let mut app = fresh_app(root);
    app.update_viewport_size(W, H);
    app
}

fn mouse(app: &mut AppState, kind: MouseEventKind, column: u16, row: u16) {
    mouse_mod(app, kind, column, row, KeyModifiers::NONE);
}

fn mouse_mod(app: &mut AppState, kind: MouseEventKind, column: u16, row: u16, m: KeyModifiers) {
    app.handle_mouse_event(MouseEvent {
        kind,
        column,
        row,
        modifiers: m,
    });
}

const LEFT: MouseEventKind = MouseEventKind::Down(MouseButton::Left);
const RIGHT: MouseEventKind = MouseEventKind::Down(MouseButton::Right);
const UP: MouseEventKind = MouseEventKind::ScrollUp;
const DOWN: MouseEventKind = MouseEventKind::ScrollDown;
const MOVED: MouseEventKind = MouseEventKind::Moved;

/// (path bar, tree, file list) rectangles for the app's current layout.
fn panes(app: &AppState) -> (Rect, Option<Rect>, Rect) {
    tui::main_panes_geometry(
        area(),
        !app.search_query.is_empty(),
        app.ui_state.ui_show_tree,
        app.layout.tree_width_percent,
    )
}

/// Screen position of file-list entry `vis_idx` (single column, first page).
fn file_row(app: &AppState, vis_idx: usize) -> (u16, u16) {
    let (_, _, list) = panes(app);
    let pos = (list.x + 2, list.y + 1 + vis_idx as u16);
    let hit = tui::hit_test_file_list(
        list,
        app.visible_indices.len(),
        app.focused_vis_idx,
        app.ui_state.ui_show_size,
        app.ui_state.ui_show_time,
        app.layout.min_col_width,
        pos,
    );
    assert_eq!(hit, Some(vis_idx), "layout assumption for row {vis_idx}");
    pos
}

/// `a.txt` .. `e.txt`.
fn five_files(root: &Path) {
    for n in ["a", "b", "c", "d", "e"] {
        fs::write(root.join(format!("{n}.txt")), n).unwrap();
    }
}

fn input_kind(app: &AppState) -> &InputPromptKind {
    match &app.overlay {
        OverlayMode::Input { kind, .. } => kind,
        other => panic!("expected an input prompt, got {other:?}"),
    }
}

/// Row of popup item `idx` when the list is scrolled to `cursor`'s page.
fn popup_item_pos(total: usize, cursor: usize, idx: usize) -> (u16, u16) {
    let (popup, start, page) = tui::popup_list_geometry(area(), total, cursor);
    assert!(idx >= start && idx - start < page);
    (popup.x + 3, popup.y + 1 + (idx - start) as u16)
}

// --- Main panes ----------------------------------------------------------------------------

#[test]
fn menubar_row_click_opens_a_category() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());

    mouse(&mut app, LEFT, 50, 0);
    assert_eq!(app.overlay, OverlayMode::None, "no category there");
    mouse(&mut app, RIGHT, 12, 0);
    assert_eq!(app.overlay, OverlayMode::None, "right click does nothing");
    mouse(&mut app, LEFT, 12, 0);
    assert_eq!(
        app.overlay,
        OverlayMode::MenuBar {
            cat_idx: 1,
            item_idx: 0
        }
    );
}

#[test]
fn path_bar_click_opens_the_path_or_search_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());

    let (bar, _, _) = panes(&app);
    mouse(&mut app, LEFT, 5, bar.y);
    assert_eq!(input_kind(&app), &InputPromptKind::PathBar);

    app.overlay = OverlayMode::None;
    app.search_query = "x".into();
    let (bar, _, _) = panes(&app);
    assert_eq!(bar.height, 2);
    mouse(&mut app, LEFT, 5, bar.y + 1);
    assert_eq!(input_kind(&app), &InputPromptKind::Search);
}

#[test]
fn file_list_scroll_moves_by_three_and_clamps() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    app.focus_pane = FocusPane::Tree;
    let (_, _, list) = panes(&app);
    let (x, y) = (list.x + 2, list.y + 2);

    mouse(&mut app, DOWN, x, y);
    assert_eq!(app.focus_pane, FocusPane::FileList);
    assert_eq!(app.focused_vis_idx, 3);
    mouse(&mut app, DOWN, x, y);
    assert_eq!(app.focused_vis_idx, 4);
    mouse(&mut app, UP, x, y);
    assert_eq!(app.focused_vis_idx, 1);
    mouse(&mut app, UP, x, y);
    assert_eq!(app.focused_vis_idx, 0);
}

#[test]
fn file_list_ctrl_click_toggles_and_shift_click_selects_a_range() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let root = tmp.path();
    let mut app = app_in(root);

    let (x, y) = file_row(&app, 1);
    mouse_mod(&mut app, LEFT, x, y, KeyModifiers::CONTROL);
    assert_eq!(app.focused_vis_idx, 1);
    assert!(app.selected_paths.contains(&root.join("b.txt")));
    mouse_mod(&mut app, LEFT, x, y, KeyModifiers::CONTROL);
    assert!(app.selected_paths.is_empty(), "ctrl-click again deselects");
    assert!(
        app.last_click.is_none(),
        "ctrl-click never counts as a double click"
    );

    let (x, y) = file_row(&app, 3);
    mouse_mod(&mut app, LEFT, x, y, KeyModifiers::SHIFT);
    assert_eq!(app.focused_vis_idx, 3);
    for name in ["b.txt", "c.txt", "d.txt"] {
        assert!(app.selected_paths.contains(&root.join(name)), "{name}");
    }
    assert_eq!(app.selected_paths.len(), 3);

    // A plain click clears the selection and focuses the clicked entry.
    let (x, y) = file_row(&app, 0);
    mouse(&mut app, LEFT, x, y);
    assert!(app.selected_paths.is_empty());
    assert_eq!(app.focused_vis_idx, 0);
}

#[test]
fn file_list_click_below_the_entries_clears_selection() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    app.execute_action(ActionId::SelectAll);
    app.focused_vis_idx = 2;

    let (_, _, list) = panes(&app);
    mouse(&mut app, LEFT, list.x + 2, list.y + list.height - 3);
    assert!(app.selected_paths.is_empty());
    assert_eq!(app.focused_vis_idx, 2, "focus stays where it was");
}

#[test]
fn file_list_double_click_on_a_file_opens_the_viewer() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    let (x, y) = file_row(&app, 2);
    mouse(&mut app, LEFT, x, y);
    assert_eq!(app.overlay, OverlayMode::None);
    mouse(&mut app, LEFT, x, y);
    assert!(matches!(app.overlay, OverlayMode::Viewer { .. }));
}

#[test]
fn file_list_right_click_keeps_a_selection_it_lands_in() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let root = tmp.path();
    let mut app = app_in(root);
    app.execute_action(ActionId::SelectAll);

    let (x, y) = file_row(&app, 2);
    mouse(&mut app, RIGHT, x, y);
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });
    assert_eq!(app.focused_vis_idx, 2);
    assert_eq!(app.selected_paths.len(), 5);

    // Right-click outside the selection drops it.
    app.overlay = OverlayMode::None;
    app.selected_paths.clear();
    app.selected_paths.insert(root.join("a.txt"));
    let (x, y) = file_row(&app, 4);
    mouse(&mut app, RIGHT, x, y);
    assert!(app.selected_paths.is_empty());
    assert_eq!(app.focused_vis_idx, 4);
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });
}

/// Tree rect, and the screen row of tree node `idx` once the cursor is on it.
fn tree_row(app: &mut AppState, idx: usize) -> (Rect, u16) {
    let (_, tree, _) = panes(app);
    let t = tree.expect("tree pane is shown by default");
    let page = t.height.saturating_sub(2).max(1) as usize;
    app.tree_cursor = idx;
    let start = (idx / page) * page;
    (t, t.y + 1 + (idx - start) as u16)
}

fn tree_index(app: &AppState, path: &Path) -> usize {
    let key = normalize_tree_key(path);
    app.tree_nodes
        .iter()
        .position(|n| n.remote.is_none() && normalize_tree_key(&n.path) == key)
        .expect("node in tree")
}

#[test]
fn tree_caret_click_toggles_and_name_click_navigates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("child")).unwrap();
    let mut app = app_in(root);
    app.rebuild_tree();

    let idx = tree_index(&app, root);
    let depth = app.tree_nodes[idx].depth as u16;
    let (t, row) = tree_row(&mut app, idx);
    assert!(app.tree_nodes[idx].expanded);

    // The caret area is the first depth*2+2 columns inside the border.
    mouse(&mut app, LEFT, t.x + 1 + depth * 2, row);
    assert_eq!(app.focus_pane, FocusPane::Tree);
    assert_eq!(app.tree_cursor, idx);
    assert!(!app.tree_nodes[idx].expanded, "caret click collapses");
    assert_eq!(app.current_dir, root, "caret click does not navigate");

    let child = root.join("child");
    app.navigate_to(child.clone());
    app.focus_pane = FocusPane::Tree;
    let idx = tree_index(&app, root);
    let (t, row) = tree_row(&mut app, idx);
    app.last_click = None;
    mouse(&mut app, LEFT, t.x + t.width - 3, row);
    assert_eq!(app.focus_pane, FocusPane::FileList);
    assert_eq!(
        normalize_tree_key(&app.current_dir),
        normalize_tree_key(root)
    );
}

#[test]
fn tree_scroll_and_right_click() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let mut app = app_in(root);
    app.rebuild_tree();
    let idx = tree_index(&app, root);
    let (t, row) = tree_row(&mut app, idx);
    let last = app.tree_nodes.len() - 1;

    mouse(&mut app, UP, t.x + 2, row);
    assert_eq!(app.focus_pane, FocusPane::Tree);
    assert_eq!(app.tree_cursor, idx.saturating_sub(3));
    for _ in 0..(last / 3 + 2) {
        mouse(&mut app, DOWN, t.x + 2, row);
    }
    assert_eq!(app.tree_cursor, last, "scroll down clamps");

    let (t, row) = tree_row(&mut app, idx);
    app.focus_pane = FocusPane::FileList;
    mouse(&mut app, RIGHT, t.x + 3, row);
    assert_eq!(app.focus_pane, FocusPane::Tree);
    assert_eq!(app.tree_cursor, idx);
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });
}

#[test]
fn mouse_is_ignored_during_a_transfer() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    let (_tx, rx) = mpsc::channel();
    app.active_transfer = Some(ActiveTransfer {
        cancel_flag: Arc::new(AtomicBool::new(false)),
        rx,
        current_state: TransferProgressState {
            title: String::new(),
            current_file: String::new(),
            current_file_bytes: 0,
            current_file_total: 0,
            total_bytes_transferred: 0,
            total_bytes: 0,
            files_transferred: 0,
            total_files: 0,
            is_cancelling: false,
        },
        conflict_prompt: None,
    });
    let (x, y) = file_row(&app, 2);
    mouse(&mut app, LEFT, x, y);
    mouse(&mut app, LEFT, 12, 0);
    assert_eq!(app.focused_vis_idx, 0);
    assert_eq!(app.overlay, OverlayMode::None);
}

// --- Overlays ------------------------------------------------------------------------------

#[test]
fn input_prompt_closes_on_a_click_outside_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());
    app.execute_action(ActionId::NewFile);
    let (popup, _, _) = tui::popup_list_geometry(area(), 1, 0);

    mouse(&mut app, LEFT, popup.x + 2, popup.y + 1);
    assert!(matches!(app.overlay, OverlayMode::Input { .. }));
    mouse(&mut app, UP, 0, H - 1);
    assert!(
        matches!(app.overlay, OverlayMode::Input { .. }),
        "scroll is ignored"
    );
    mouse(&mut app, RIGHT, 0, H - 1);
    assert_eq!(app.overlay, OverlayMode::None);

    app.execute_action(ActionId::NewFile);
    mouse(&mut app, LEFT, 0, H - 1);
    assert_eq!(app.overlay, OverlayMode::None);
}

#[test]
fn action_menu_scroll_hover_and_click() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    let total = ACTION_MENU_FIXED_COUNT + app.external_apps.len();
    app.overlay = OverlayMode::ActionMenu { cursor: 0 };

    mouse(&mut app, UP, 0, 0);
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });
    for _ in 0..total + 3 {
        mouse(&mut app, DOWN, 0, 0);
    }
    assert_eq!(
        app.overlay,
        OverlayMode::ActionMenu { cursor: total - 1 },
        "scroll clamps"
    );

    app.overlay = OverlayMode::ActionMenu { cursor: 0 };
    let (x, y) = popup_item_pos(total, 0, 3);
    mouse(&mut app, MOVED, x, y);
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 3 });

    // Item 8 is "New Folder".
    let (x, y) = popup_item_pos(total, 3, 8);
    mouse(&mut app, LEFT, x, y);
    assert_eq!(input_kind(&app), &InputPromptKind::NewFolder);

    app.overlay = OverlayMode::ActionMenu { cursor: 0 };
    mouse(&mut app, LEFT, 0, H - 1);
    assert_eq!(app.overlay, OverlayMode::None, "click outside closes");
    app.overlay = OverlayMode::ActionMenu { cursor: 0 };
    let (x, y) = popup_item_pos(total, 0, 2);
    mouse(&mut app, RIGHT, x, y);
    assert_eq!(app.overlay, OverlayMode::None, "right click closes");
    assert!(app.clipboard.is_none(), "right click does not run the item");
}

#[test]
fn sort_menu_click_applies_the_sort() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    app.overlay = OverlayMode::SortMenu { cursor: 0 };

    for _ in 0..12 {
        mouse(&mut app, DOWN, 0, 0);
    }
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 7 });
    mouse(&mut app, UP, 0, 0);
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 6 });

    app.overlay = OverlayMode::SortMenu { cursor: 0 };
    let (x, y) = popup_item_pos(8, 0, 1);
    mouse(&mut app, MOVED, x, y);
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 1 });
    mouse(&mut app, LEFT, x, y);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.ui_state.sort_key, "name");
    assert_eq!(app.ui_state.sort_order, "desc");
    assert_eq!(app.status_message, "Sorted by: Name (Desc)");
    assert_eq!(app.focused_entry().unwrap().name, "e.txt");

    app.overlay = OverlayMode::SortMenu { cursor: 0 };
    mouse(&mut app, LEFT, 0, H - 1);
    assert_eq!(app.overlay, OverlayMode::None);
    app.overlay = OverlayMode::SortMenu { cursor: 0 };
    mouse(&mut app, RIGHT, x, y);
    assert_eq!(app.overlay, OverlayMode::None);
}

fn list_popup(items: Vec<String>, cursor: usize) -> OverlayMode {
    OverlayMode::ListPopup {
        title: "History".into(),
        items,
        cursor,
        is_jump_list: false,
    }
}

#[test]
fn list_popup_scroll_hover_and_click_navigates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("one")).unwrap();
    fs::create_dir(root.join("two")).unwrap();
    let items: Vec<String> = ["one", "two"]
        .iter()
        .map(|n| root.join(n).to_string_lossy().into_owned())
        .collect();
    let mut app = app_in(root);

    app.overlay = list_popup(items.clone(), 0);
    mouse(&mut app, DOWN, 0, 0);
    mouse(&mut app, DOWN, 0, 0);
    assert_eq!(app.overlay, list_popup(items.clone(), 1));
    mouse(&mut app, UP, 0, 0);
    assert_eq!(app.overlay, list_popup(items.clone(), 0));

    let (x, y) = popup_item_pos(2, 0, 1);
    mouse(&mut app, MOVED, x, y);
    assert_eq!(app.overlay, list_popup(items.clone(), 1));
    mouse(&mut app, LEFT, x, y);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.current_dir, root.join("two"));

    app.overlay = list_popup(items.clone(), 0);
    mouse(&mut app, LEFT, 0, H - 1);
    assert_eq!(app.overlay, OverlayMode::None);
    app.overlay = list_popup(items, 0);
    mouse(&mut app, RIGHT, x, y);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.current_dir, root.join("two"));
}

#[test]
fn list_popup_with_no_items() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());
    app.overlay = list_popup(Vec::new(), 0);
    mouse(&mut app, DOWN, 0, 0);
    assert_eq!(app.overlay, list_popup(Vec::new(), 0));
    let (x, y) = popup_item_pos(1, 0, 0);
    mouse(&mut app, MOVED, x, y);
    assert_eq!(app.overlay, list_popup(Vec::new(), 0));
    mouse(&mut app, LEFT, x, y);
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.current_dir, tmp.path());
}

fn menubar(cat_idx: usize, item_idx: usize) -> OverlayMode {
    OverlayMode::MenuBar { cat_idx, item_idx }
}

#[test]
fn menubar_category_row_switches_and_closes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());
    app.overlay = menubar(1, 2);

    mouse(&mut app, MOVED, 12, 0);
    assert_eq!(
        app.overlay,
        menubar(1, 2),
        "hovering the open category keeps it"
    );
    mouse(&mut app, MOVED, 22, 0);
    assert_eq!(
        app.overlay,
        menubar(2, 0),
        "hovering another category switches"
    );
    mouse(&mut app, LEFT, 32, 0);
    assert_eq!(app.overlay, menubar(3, 0));
    mouse(&mut app, LEFT, 32, 0);
    assert_eq!(
        app.overlay,
        OverlayMode::None,
        "clicking the open category closes"
    );

    app.overlay = menubar(0, 0);
    mouse(&mut app, MOVED, 60, 0);
    assert_eq!(app.overlay, menubar(0, 0));
    mouse(&mut app, LEFT, 60, 0);
    assert_eq!(app.overlay, OverlayMode::None);
}

#[test]
fn menubar_dropdown_scroll_hover_and_click() {
    let tmp = tempfile::tempdir().unwrap();
    five_files(tmp.path());
    let mut app = app_in(tmp.path());
    // View menu: Tree, Hidden, Size, Time, Sort.
    let total = 5;
    let rect = tui::menubar_dropdown_rect(area(), 2, total);
    app.overlay = menubar(2, 0);

    for _ in 0..8 {
        mouse(&mut app, DOWN, 0, 10);
    }
    assert_eq!(app.overlay, menubar(2, total - 1));
    mouse(&mut app, UP, 0, 10);
    assert_eq!(app.overlay, menubar(2, total - 2));

    mouse(&mut app, MOVED, rect.x + 2, rect.y + 1 + 1);
    assert_eq!(app.overlay, menubar(2, 1));
    mouse(&mut app, LEFT, rect.x + 2, rect.y + 1 + 4);
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 0 });

    app.overlay = menubar(2, 0);
    mouse(&mut app, LEFT, W - 1, H - 1);
    assert_eq!(app.overlay, OverlayMode::None);
    app.overlay = menubar(2, 0);
    mouse(&mut app, RIGHT, rect.x + 2, rect.y + 1);
    assert_eq!(app.overlay, OverlayMode::None);
}

fn viewer_state(app: &AppState) -> (bool, usize, usize) {
    match &app.overlay {
        OverlayMode::Viewer {
            preview_mode,
            scroll_y,
            scroll_x,
            ..
        } => (*preview_mode, *scroll_y, *scroll_x),
        other => panic!("expected the viewer, got {other:?}"),
    }
}

fn set_viewer_scroll(app: &mut AppState, y: usize, x: usize) {
    if let OverlayMode::Viewer {
        scroll_y, scroll_x, ..
    } = &mut app.overlay
    {
        *scroll_y = y;
        *scroll_x = x;
    }
}

#[test]
fn viewer_scrolls_by_three_and_clamps() {
    let tmp = tempfile::tempdir().unwrap();
    let text: String = (1..=5).map(|i| format!("line {i}\n")).collect();
    fs::write(tmp.path().join("t.txt"), text).unwrap();
    let mut app = app_in(tmp.path());
    focus(&mut app, "t.txt");
    app.execute_action(ActionId::Open);
    let (preview, _, _) = viewer_state(&app);

    mouse(&mut app, DOWN, 5, 5);
    assert_eq!(viewer_state(&app).1, 3);
    mouse(&mut app, DOWN, 5, 5);
    let (_, y, _) = viewer_state(&app);
    mouse(&mut app, DOWN, 5, 5);
    assert_eq!(viewer_state(&app).1, y, "clamped at the end");
    mouse(&mut app, UP, 5, 5);
    mouse(&mut app, UP, 5, 5);
    mouse(&mut app, UP, 5, 5);
    assert_eq!(viewer_state(&app).1, 0);

    mouse(&mut app, RIGHT, 5, 5);
    assert_eq!(
        viewer_state(&app).0,
        preview,
        "right click does nothing on plain text"
    );
    mouse(&mut app, LEFT, 5, 5);
    assert!(matches!(app.overlay, OverlayMode::Viewer { .. }));
}

#[test]
fn viewer_right_click_toggles_markdown_preview() {
    // The preview comes from the external `leaf.exe`, so build the viewer state directly
    // instead of depending on it being installed.
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());
    let raw: Vec<String> = (0..10)
        .map(|i| format!("raw {i}{}", if i % 3 == 0 { " hit" } else { "" }))
        .collect();
    let preview: Vec<String> = vec!["p0".into(), "p1 hit".into(), "p2".into(), "p3".into()];
    app.overlay = OverlayMode::Viewer {
        path: tmp.path().join("doc.md"),
        title: "doc.md".into(),
        is_markdown: true,
        preview_mode: false,
        raw_lines: raw,
        preview_lines: Some(preview),
        scroll_y: 8,
        scroll_x: 7,
        search_query: Some("hit".into()),
        search_input: None,
        matching_lines: vec![0, 3, 6, 9],
        match_cursor: Some(3),
    };

    mouse(&mut app, RIGHT, 5, 5);
    let (mode, y, x) = viewer_state(&app);
    assert!(mode, "switches to the preview");
    assert_eq!(y, 3, "scroll clamps to the shorter preview");
    assert_eq!(x, 0, "horizontal scroll resets");
    match &app.overlay {
        OverlayMode::Viewer {
            matching_lines,
            match_cursor,
            ..
        } => {
            assert_eq!(matching_lines, &vec![1], "search re-runs on the preview");
            assert_eq!(*match_cursor, Some(0));
        }
        other => panic!("unexpected overlay {other:?}"),
    }

    set_viewer_scroll(&mut app, 2, 5);
    mouse(&mut app, RIGHT, 5, 5);
    let (mode, y, x) = viewer_state(&app);
    assert!(!mode, "switches back to the raw text");
    assert_eq!(y, 2, "scroll is kept while it fits");
    assert_eq!(x, 0);
    match &app.overlay {
        OverlayMode::Viewer {
            matching_lines,
            match_cursor,
            ..
        } => {
            assert_eq!(matching_lines, &vec![0, 3, 6, 9]);
            assert_eq!(*match_cursor, Some(1), "first match at or below the scroll");
        }
        other => panic!("unexpected overlay {other:?}"),
    }

    // Without a rendered preview (no `leaf.exe`), right click does nothing.
    if let OverlayMode::Viewer { preview_lines, .. } = &mut app.overlay {
        *preview_lines = None;
    }
    set_viewer_scroll(&mut app, 2, 5);
    mouse(&mut app, RIGHT, 5, 5);
    assert_eq!(viewer_state(&app), (false, 2, 5));
}

#[test]
fn connection_dialogs_close_on_right_click_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = app_in(tmp.path());
    let overlays = [
        OverlayMode::ConnectionManager { cursor: 0 },
        OverlayMode::ConnectionEdit {
            is_new: true,
            profile: ConnectionProfile::default(),
            cursor: 0,
            password_buffer: String::new(),
        },
        OverlayMode::PasswordPrompt {
            profile_id: "p".into(),
            title: "t".into(),
            buffer: String::new(),
            save_password: false,
        },
        OverlayMode::HostKeyConfirm {
            endpoint: "h:22".into(),
            fingerprint: "SHA256:x".into(),
        },
    ];
    for overlay in overlays {
        app.overlay = overlay.clone();
        mouse(&mut app, LEFT, 0, H - 1);
        mouse(&mut app, DOWN, 0, H - 1);
        assert_eq!(app.overlay, overlay);
        mouse(&mut app, RIGHT, 5, 5);
        assert_eq!(app.overlay, OverlayMode::None, "{overlay:?}");
    }
}
