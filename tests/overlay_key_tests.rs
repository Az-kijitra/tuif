//! Characterization tests for keyboard handling inside overlays (dialogs, prompts, menus).
//!
//! They pin the current behavior of `AppState::handle_overlay_key` so that splitting it into
//! per-overlay handlers can be checked for regressions. Each test drives the app only through
//! `handle_key_event`, the same entry point the main loop uses.

mod common;

use common::*;
use crossterm::event::KeyCode;
use tuif::app::{InputPromptKind, OverlayMode};

fn sample_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("dir_a")).unwrap();
    std::fs::write(root.join("file1.txt"), b"one").unwrap();
    std::fs::write(root.join("file2.md"), b"two").unwrap();
    tmp
}

fn input_buffer(overlay: &OverlayMode) -> &str {
    match overlay {
        OverlayMode::Input { buffer, .. } => buffer,
        other => panic!("expected an Input overlay, got {other:?}"),
    }
}

// ---------------------------------------------------------------- Input: delete confirmation

#[test]
fn confirm_delete_n_and_esc_keep_the_file() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    focus(&mut app, "file1.txt");

    for cancel in [key(KeyCode::Char('n')), key(KeyCode::Esc)] {
        app.handle_key_event(shift('D'));
        assert!(matches!(
            &app.overlay,
            OverlayMode::Input {
                kind: InputPromptKind::ConfirmDelete {
                    permanent: true,
                    ..
                },
                ..
            }
        ));
        app.handle_key_event(cancel);
        assert_eq!(app.overlay, OverlayMode::None);
        assert!(tmp.path().join("file1.txt").exists());
    }
}

#[test]
fn confirm_delete_y_deletes_permanently_and_reports() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    focus(&mut app, "file1.txt");

    app.handle_key_event(shift('D'));
    app.handle_key_event(key(KeyCode::Char('y')));

    assert_eq!(app.overlay, OverlayMode::None);
    assert!(!tmp.path().join("file1.txt").exists());
    assert!(
        app.status_message.contains("1 succeeded / 0 failed"),
        "{}",
        app.status_message
    );
    assert!(!visible_names(&app).contains(&"file1.txt".to_string()));
}

// ---------------------------------------------------------------- Input: text prompts

#[test]
fn rename_prompt_prefills_name_edits_and_commits() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    focus(&mut app, "file1.txt");

    app.handle_key_event(key(KeyCode::Char('r')));
    assert_eq!(input_buffer(&app.overlay), "file1.txt");

    for _ in 0.."file1.txt".len() {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    assert_eq!(input_buffer(&app.overlay), "");
    type_text(&mut app, "renamed.txt");
    assert_eq!(input_buffer(&app.overlay), "renamed.txt");

    app.handle_key_event(key(KeyCode::Enter));
    assert_eq!(app.overlay, OverlayMode::None);
    assert!(tmp.path().join("renamed.txt").exists());
    assert!(!tmp.path().join("file1.txt").exists());
}

#[test]
fn rename_prompt_esc_cancels_without_renaming() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    focus(&mut app, "file1.txt");

    app.handle_key_event(key(KeyCode::Char('r')));
    type_text(&mut app, "-x");
    app.handle_key_event(key(KeyCode::Esc));

    assert_eq!(app.overlay, OverlayMode::None);
    assert!(tmp.path().join("file1.txt").exists());
    assert!(!tmp.path().join("file1.txt-x").exists());
}

#[test]
fn prompt_ignores_ctrl_and_alt_characters() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(shift('A'));
    type_text(&mut app, "ab");
    app.handle_key_event(ctrl('z'));
    app.handle_key_event(key_mod(
        KeyCode::Char('q'),
        crossterm::event::KeyModifiers::ALT,
    ));
    assert_eq!(input_buffer(&app.overlay), "ab");
}

#[test]
fn new_folder_prompt_creates_directory() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(shift('A'));
    assert!(matches!(
        &app.overlay,
        OverlayMode::Input {
            kind: InputPromptKind::NewFolder,
            ..
        }
    ));
    type_text(&mut app, "made_by_test");
    app.handle_key_event(key(KeyCode::Enter));

    assert_eq!(app.overlay, OverlayMode::None);
    assert!(
        tmp.path().join("made_by_test").is_dir(),
        "status={} focus={:?} entries={:?}",
        app.status_message,
        app.focus_pane,
        visible_names(&app)
    );
}

// ---------------------------------------------------------------- Input: incremental search

#[test]
fn search_filters_while_typing_and_esc_clears() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    assert_eq!(app.visible_indices.len(), 3);

    app.handle_key_event(key(KeyCode::Char('/')));
    type_text(&mut app, "file1");
    assert_eq!(app.search_query, "file1");
    assert_eq!(visible_names(&app), vec!["file1.txt"]);

    app.handle_key_event(key(KeyCode::Backspace));
    assert_eq!(app.search_query, "file");
    assert_eq!(app.visible_indices.len(), 2);

    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.search_query, "");
    assert_eq!(app.visible_indices.len(), 3);
}

#[test]
fn search_enter_keeps_the_filter() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(key(KeyCode::Char('/')));
    type_text(&mut app, "md");
    app.handle_key_event(key(KeyCode::Enter));

    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(visible_names(&app), vec!["file2.md"]);
}

#[test]
fn search_ctrl_r_toggles_regex_and_keeps_the_prompt_open() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    assert!(!app.search_regex);

    app.handle_key_event(key(KeyCode::Char('/')));
    type_text(&mut app, "^f.*1");
    assert_eq!(app.visible_indices.len(), 0, "literal match finds nothing");

    app.handle_key_event(ctrl('r'));
    assert!(app.search_regex);
    assert_eq!(input_buffer(&app.overlay), "^f.*1");
    assert_eq!(visible_names(&app), vec!["file1.txt"]);
}

// ---------------------------------------------------------------- SortMenu

#[test]
fn sort_menu_moves_cursor_within_bounds() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(key(KeyCode::Char('s')));
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 0 });
    app.handle_key_event(key(KeyCode::Char('k')));
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 0 });
    for _ in 0..20 {
        app.handle_key_event(key(KeyCode::Char('j')));
    }
    assert_eq!(app.overlay, OverlayMode::SortMenu { cursor: 7 });
    app.handle_key_event(key(KeyCode::Char('q')));
    assert_eq!(app.overlay, OverlayMode::None);
}

#[test]
fn sort_menu_enter_applies_name_descending() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(key(KeyCode::Char('s')));
    app.handle_key_event(key(KeyCode::Char('j'))); // Name (Desc)
    app.handle_key_event(key(KeyCode::Enter));

    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.ui_state.sort_key, "name");
    assert_eq!(app.ui_state.sort_order, "desc");
    assert_eq!(app.status_message, "Sorted by: Name (Desc)");
}

// ---------------------------------------------------------------- ListPopup (history)

#[test]
fn history_popup_navigates_deletes_and_opens() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    let a = tmp.path().join("dir_a");
    app.nav_data.visit_history = vec![
        a.to_string_lossy().into_owned(),
        tmp.path().to_string_lossy().into_owned(),
        "C:\\does-not-matter".to_string(),
    ];

    app.handle_key_event(ctrl('y'));
    let OverlayMode::ListPopup {
        items,
        is_jump_list,
        ..
    } = app.overlay.clone()
    else {
        panic!("expected ListPopup, got {:?}", app.overlay);
    };
    assert!(!is_jump_list);
    assert_eq!(items.len(), 3);

    // Delete the last entry: cursor clamps back onto the new last item.
    app.handle_key_event(key(KeyCode::Char('j')));
    app.handle_key_event(key(KeyCode::Char('j')));
    app.handle_key_event(key(KeyCode::Char('j')));
    app.handle_key_event(key(KeyCode::Char('d')));
    let OverlayMode::ListPopup { items, cursor, .. } = app.overlay.clone() else {
        panic!("popup should stay open after delete");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(cursor, 1);
    assert_eq!(app.nav_data.visit_history.len(), 2);

    // Enter on the first item navigates there.
    app.handle_key_event(key(KeyCode::Char('k')));
    app.handle_key_event(key(KeyCode::Enter));
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.current_dir, a);
}

// ---------------------------------------------------------------- ActionMenu

#[test]
fn action_menu_bounds_and_open_item() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    focus(&mut app, "dir_a");
    let total = tuif::app::ACTION_MENU_FIXED_COUNT + app.external_apps.len();

    app.handle_key_event(key(KeyCode::Char(';')));
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: 0 });
    for _ in 0..(total + 5) {
        app.handle_key_event(key(KeyCode::Down));
    }
    assert_eq!(app.overlay, OverlayMode::ActionMenu { cursor: total - 1 });
    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayMode::None);

    // Item 0 is "Open": entering the focused folder.
    app.handle_key_event(key(KeyCode::Char(';')));
    app.handle_key_event(key(KeyCode::Enter));
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.current_dir, tmp.path().join("dir_a"));
}

// ---------------------------------------------------------------- MenuBar

#[test]
fn menubar_wraps_categories_and_clamps_items() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(key(KeyCode::F(10)));
    assert_eq!(
        app.overlay,
        OverlayMode::MenuBar {
            cat_idx: 0,
            item_idx: 0
        }
    );
    app.handle_key_event(key(KeyCode::Char('h')));
    assert_eq!(
        app.overlay,
        OverlayMode::MenuBar {
            cat_idx: 3,
            item_idx: 0
        }
    );
    app.handle_key_event(key(KeyCode::Char('l')));
    app.handle_key_event(key(KeyCode::Char('l'))); // category 1 has 6 items
    for _ in 0..20 {
        app.handle_key_event(key(KeyCode::Char('j')));
    }
    assert_eq!(
        app.overlay,
        OverlayMode::MenuBar {
            cat_idx: 1,
            item_idx: 5
        }
    );
    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayMode::None);
}

// ---------------------------------------------------------------- Connection manager / editor

#[test]
fn connection_manager_opens_editor_and_esc_returns() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    app.remote_profiles.clear();

    app.handle_key_event(ctrl('s'));
    assert_eq!(app.overlay, OverlayMode::ConnectionManager { cursor: 0 });
    app.handle_key_event(key(KeyCode::Char('j')));
    assert_eq!(app.overlay, OverlayMode::ConnectionManager { cursor: 0 });

    app.handle_key_event(key(KeyCode::Char('n')));
    let OverlayMode::ConnectionEdit {
        is_new,
        cursor,
        profile,
        ..
    } = app.overlay.clone()
    else {
        panic!("expected ConnectionEdit, got {:?}", app.overlay);
    };
    assert!(is_new);
    assert_eq!(cursor, 0);
    assert_eq!(profile.port, 0, "port starts empty for new profiles");

    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayMode::ConnectionManager { cursor: 0 });
    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayMode::None);
    assert!(app.remote_profiles.is_empty(), "Esc must not save");
}

#[test]
fn connection_editor_fields_follow_the_cursor() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(ctrl('s'));
    app.handle_key_event(key(KeyCode::Char('n')));
    type_text(&mut app, "srv"); // 0: name
    app.handle_key_event(key(KeyCode::Tab));
    type_text(&mut app, "example.com"); // 1: host
    app.handle_key_event(key(KeyCode::Backspace));
    app.handle_key_event(key(KeyCode::Tab));
    type_text(&mut app, "x"); // 2: port ignores non-digits
    app.handle_key_event(key(KeyCode::Tab));
    type_text(&mut app, "me"); // 3: user
    app.handle_key_event(key(KeyCode::Tab));
    type_text(&mut app, "pw"); // 4: password buffer
    app.handle_key_event(key(KeyCode::Tab));
    app.handle_key_event(key(KeyCode::Char(' '))); // 5: toggles save_password
    for _ in 0..5 {
        app.handle_key_event(key(KeyCode::Tab)); // clamps at 6
    }
    type_text(&mut app, "/srv"); // 6: initial dir
    app.handle_key_event(key(KeyCode::BackTab));

    let OverlayMode::ConnectionEdit {
        profile,
        cursor,
        password_buffer,
        ..
    } = app.overlay.clone()
    else {
        panic!("expected ConnectionEdit, got {:?}", app.overlay);
    };
    assert_eq!(profile.name, "srv");
    assert_eq!(profile.host, "example.co");
    assert_eq!(profile.port, 0);
    assert_eq!(profile.user, "me");
    assert_eq!(password_buffer, "pw");
    assert!(
        profile.save_password != tuif::remote::profile::ConnectionProfile::default().save_password
    );
    assert_eq!(profile.initial_dir, "/srv");
    assert_eq!(cursor, 5);
}

// ---------------------------------------------------------------- Password / host key prompts

#[test]
fn password_prompt_edits_toggles_and_esc_cancels() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    app.overlay = OverlayMode::PasswordPrompt {
        profile_id: "p".into(),
        title: "Password".into(),
        buffer: String::new(),
        save_password: false,
    };

    type_text(&mut app, "secret");
    app.handle_key_event(key(KeyCode::Backspace));
    app.handle_key_event(key(KeyCode::Tab));
    assert_eq!(
        app.overlay,
        OverlayMode::PasswordPrompt {
            profile_id: "p".into(),
            title: "Password".into(),
            buffer: "secre".into(),
            save_password: true,
        }
    );

    app.handle_key_event(key(KeyCode::Esc));
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(app.status_message, "Connection cancelled");
}

#[test]
fn host_key_prompt_rejects_with_n() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());
    app.overlay = OverlayMode::HostKeyConfirm {
        endpoint: "example.com:22".into(),
        fingerprint: "SHA256:abc".into(),
    };

    app.handle_key_event(key(KeyCode::Char('x')));
    assert!(matches!(app.overlay, OverlayMode::HostKeyConfirm { .. }));
    app.handle_key_event(key(KeyCode::Char('n')));
    assert_eq!(app.overlay, OverlayMode::None);
    assert_eq!(
        app.status_message,
        "Host key rejected. Connection cancelled."
    );
}

/// Regression: typing "2222" used to be impossible because the field was cleared whenever it
/// read "22".
#[test]
fn connection_editor_port_accepts_repeated_digits_and_backspace() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(ctrl('s'));
    app.handle_key_event(key(KeyCode::Char('n')));
    app.handle_key_event(key(KeyCode::Tab));
    app.handle_key_event(key(KeyCode::Tab)); // 2: port
    let port = |app: &tuif::app::AppState| match &app.overlay {
        OverlayMode::ConnectionEdit { profile, .. } => profile.port,
        other => panic!("expected ConnectionEdit, got {other:?}"),
    };

    type_text(&mut app, "2222");
    assert_eq!(port(&app), 2222);
    for _ in 0..4 {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    type_text(&mut app, "65535");
    assert_eq!(port(&app), 65535);
    type_text(&mut app, "1"); // 655351 > u16::MAX is ignored
    assert_eq!(port(&app), 65535);
    for _ in 0..3 {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    assert_eq!(port(&app), 65);
    for _ in 0..2 {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    type_text(&mut app, "22");
    assert_eq!(port(&app), 22);
    type_text(&mut app, "0");
    assert_eq!(port(&app), 220);
    for _ in 0..3 {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    assert_eq!(port(&app), 0, "empty field");
    type_text(&mut app, "22");
    assert_eq!(port(&app), 22);
}

/// Regression: creating a local folder always failed with `InvalidKind`.
#[test]
fn new_folder_via_action_menu_path_is_created_too() {
    let tmp = sample_dir();
    let mut app = fresh_app(tmp.path());

    app.handle_key_event(shift('A'));
    type_text(&mut app, "second");
    app.handle_key_event(key(KeyCode::Enter));
    assert!(tmp.path().join("second").is_dir(), "{}", app.status_message);
    assert_eq!(app.status_message, "Created folder: second");
}
