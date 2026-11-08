//! Shared helpers for integration tests.
//!
//! `isolate_config()` points `TUIF_CONFIG_DIR` at a throwaway directory so tests never read the
//! developer's real `%APPDATA%\tuif` (custom keymaps, themes) nor write to it (navigation history,
//! UI state, connection profiles, trusted host keys). The override is honored in debug builds
//! only, which is what `cargo test` produces.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tuif::app::AppState;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sets `TUIF_CONFIG_DIR` to a per-process temp directory (once) and returns it.
pub fn isolate_config() -> &'static Path {
    CONFIG_DIR.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("tuif-test-config-")
            .tempdir()
            .expect("create temp config dir")
            .keep();
        // Set before any AppState is created; every test calls this first.
        std::env::set_var("TUIF_CONFIG_DIR", &dir);
        dir
    })
}

/// A fresh `AppState` rooted at `dir` with default settings and keymap.
pub fn fresh_app(dir: &Path) -> AppState {
    isolate_config();
    let mut app = AppState::new(dir.to_path_buf());
    app.ui_state = tuif::shared::schema::ui_state::UiStateData::default();
    app.reload_dir();
    app
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub fn shift(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT)
}

/// Sends each character of `text` as a plain key press.
pub fn type_text(app: &mut AppState, text: &str) {
    for c in text.chars() {
        app.handle_key_event(key(KeyCode::Char(c)));
    }
}

/// Moves the file-list cursor onto the entry named `name`.
pub fn focus(app: &mut AppState, name: &str) {
    app.handle_key_event(key(KeyCode::Home));
    for _ in 0..app.visible_indices.len() {
        if app.focused_entry().map(|e| e.name.as_str()) == Some(name) {
            return;
        }
        app.handle_key_event(key(KeyCode::Char('j')));
    }
    panic!("entry {name} not found");
}

/// Names of the currently visible entries, in display order.
pub fn visible_names(app: &AppState) -> Vec<String> {
    app.visible_indices
        .iter()
        .map(|&i| app.entries[i].name.clone())
        .collect()
}
