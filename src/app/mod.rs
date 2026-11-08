//! Application state and Vim-like controller for `tuif` (SPEC-0300-0001).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use regex::RegexBuilder;

use crate::config::paths;
use crate::external_apps::{
    build_args, default_external_apps, merge_user_apps, resolve_executable, ExternalAppEntry,
    FuzzyKind, FuzzySelection, LaunchTarget, PendingFuzzySearch,
};
use crate::fs::query::{self, Entry, EntryType};
use crate::fs::win_drives;
use crate::fs_ops::{create, delete, rename, transfer};
use crate::keymap::{ActionId, KeyChord, Keymap};
use crate::remote::credential;
use crate::remote::known_hosts::{self, HostKeyStatus, KnownHosts};
use crate::remote::path as rpath;
use crate::remote::profile::{self, ConnectionProfile};
use crate::remote::session::{self, handshake, ConflictChoice, PendingHandshake, RemoteSession};
use crate::shared::schema::navigation::NavigationData;
use crate::shared::schema::ui_state::UiStateData;
use crate::viewer::{classify, text};
mod actions;
mod connection_keys;
mod keyboard;
mod menus;
mod mouse;
mod navigation;
mod paste;
mod remote;
mod tree;
mod types;
mod viewer_keys;

pub use types::*;

impl AppState {
    pub fn new(initial_dir: PathBuf) -> Self {
        let ui_state = paths::ui_state_path()
            .ok()
            .and_then(|p| crate::config::ui_state::load(&p).ok())
            .map(|r| r.data)
            .unwrap_or_default();
        let nav_data = paths::navigation_path()
            .ok()
            .and_then(|p| crate::config::navigation::load(&p).ok())
            .map(|r| r.data)
            .unwrap_or_default();
        let cfg_load = paths::config_path()
            .ok()
            .map(|p| crate::config::config_toml::load(&p))
            .unwrap_or_default();
        let (keymap, rejected) = Keymap::from_custom(&cfg_load.keymap.entries);
        let (external_apps, mut app_notes) =
            merge_user_apps(default_external_apps(), &cfg_load.external_apps);
        app_notes.extend(cfg_load.external_apps_errors);
        let theme = cfg_load.theme;
        let general = cfg_load.general;
        let layout = cfg_load.layout;

        let mut status_message = String::from("j/k:Move  Enter:Open  BS/-:Up  Tab:Tree  y/x/p:Yank/Cut/Paste  /:Search  ;:Actions  F10:Menu  q:Quit");
        if !rejected.is_empty() {
            status_message = format!("Skipped invalid keybindings: {}", rejected.join(", "));
        } else if !app_notes.is_empty() {
            status_message = format!("config.toml external_apps: {}", app_notes.join("; "));
        }

        let drives = win_drives::list_logical_drives();
        let remote_profiles = paths::connections_path()
            .ok()
            .and_then(|p| profile::load(&p).ok())
            .unwrap_or_default();
        let known_hosts = paths::known_hosts_path()
            .ok()
            .map(|p| known_hosts::load(&p))
            .unwrap_or_default();

        let mut app = Self {
            current_dir: initial_dir,
            entries: Vec::new(),
            visible_indices: Vec::new(),
            focused_vis_idx: 0,
            list_rows_per_col: 20,
            term_width: 100,
            term_height: 28,
            last_click: None,
            selected_paths: HashSet::new(),
            drives,
            expanded_dirs: HashSet::new(),
            tree_children_cache: HashMap::new(),
            tree_nodes: Vec::new(),
            tree_cursor: 0,
            focus_pane: FocusPane::FileList,
            ui_state,
            nav_data,
            keymap,
            external_apps,
            clipboard: None,
            search_query: String::new(),
            search_regex: false,
            overlay: OverlayMode::None,
            status_message,
            should_quit: false,
            should_cd_on_exit: false,
            pending_launch: None,
            pending_fuzzy: None,
            remote_profiles,
            known_hosts,
            remote_session: None,
            remote_dir: None,
            pending_handshake: None,
            active_transfer: None,
            theme,
            general,
            layout,
        };
        app.expand_ancestors_of_current_dir();
        app.reload_dir();
        app.sync_tree_cursor_to_current_dir();
        app.record_visit();
        app
    }

    pub fn save_remote_profiles(&self) {
        if let Ok(p) = paths::connections_path() {
            let _ = profile::save(&p, &self.remote_profiles);
        }
    }

    pub fn save_known_hosts(&self) {
        if let Ok(p) = paths::known_hosts_path() {
            let _ = known_hosts::save(&p, &self.known_hosts);
        }
    }

    pub fn is_remote(&self) -> bool {
        self.remote_dir.is_some()
    }

    pub fn active_display_path(&self) -> String {
        if let (Some(session), Some(rdir)) = (&self.remote_session, &self.remote_dir) {
            format!("{}{}", session.profile.url_prefix(), rdir)
        } else {
            self.current_dir.to_string_lossy().into_owned()
        }
    }

    /// Updates `list_rows_per_col`, `term_width`, and `term_height` from the current terminal dimensions.
    pub fn update_viewport_size(&mut self, term_width: u16, term_height: u16) {
        self.term_width = term_width;
        self.term_height = term_height;
        let top_rows = 1 + if self.search_query.is_empty() { 1 } else { 2 };
        let status_rows = 1;
        let main_height = term_height.saturating_sub(top_rows + status_rows);
        // Subtract 2 for the top and bottom borders of the FileList block
        self.list_rows_per_col = main_height.saturating_sub(2).max(1) as usize;
    }
}
