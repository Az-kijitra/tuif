//! Application state and Vim-like controller for `cuifiler` (SPEC-0300-0001).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use regex::RegexBuilder;

use crate::config::paths;
use crate::external_apps::{
    build_args, default_external_apps, resolve_executable, ExternalAppEntry, LaunchTarget,
};
use crate::fs::query::{self, Entry, EntryType};
use crate::fs::win_drives;
use crate::fs_ops::{create, delete, rename, transfer};
use crate::keymap::{ActionId, KeyChord, Keymap};
use crate::shared::schema::navigation::NavigationData;
use crate::shared::schema::ui_state::UiStateData;
use crate::viewer::{classify, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    FileList,
    Tree,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMode {
    Copy,
    Cut,
}

#[derive(Debug, Clone)]
pub struct FileClipboard {
    pub mode: ClipMode,
    pub items: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPromptKind {
    PathBar,
    Search,
    Rename { target: PathBuf },
    NewFile,
    NewFolder,
    ZipCreate,
    ZipExtract { zip_path: PathBuf },
    ConfirmDelete { permanent: bool, targets: Vec<PathBuf> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayMode {
    None,
    Input {
        kind: InputPromptKind,
        title: String,
        buffer: String,
    },
    SortMenu {
        cursor: usize,
    },
    ListPopup {
        title: String,
        items: Vec<String>,
        cursor: usize,
        is_jump_list: bool,
    },
    ActionMenu {
        cursor: usize,
    },
    MenuBar {
        cat_idx: usize,
        item_idx: usize,
    },
    Viewer {
        path: PathBuf,
        title: String,
        lines: Vec<String>,
        scroll_y: usize,
        scroll_x: usize,
    },
}

/// Request returned to the main loop when an external CUI tool needs terminal handoff.
#[derive(Debug, Clone)]
pub struct PendingExternalLaunch {
    pub app_name: String,
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

pub struct AppState {
    pub current_dir: PathBuf,
    pub entries: Vec<Entry>,
    pub visible_indices: Vec<usize>,
    pub focused_vis_idx: usize,
    pub list_rows_per_col: usize,
    pub selected_paths: HashSet<PathBuf>,
    pub drives: Vec<String>,
    pub expanded_dirs: HashSet<String>,
    pub tree_children_cache: HashMap<String, Vec<(String, PathBuf)>>,
    pub tree_nodes: Vec<TreeNode>,
    pub tree_cursor: usize,
    pub focus_pane: FocusPane,
    pub ui_state: UiStateData,
    pub nav_data: NavigationData,
    pub keymap: Keymap,
    pub external_apps: Vec<ExternalAppEntry>,
    pub clipboard: Option<FileClipboard>,
    pub search_query: String,
    pub search_regex: bool,
    pub overlay: OverlayMode,
    pub status_message: String,
    pub should_quit: bool,
    pub pending_launch: Option<PendingExternalLaunch>,
}

pub fn normalize_tree_key(path: &Path) -> String {
    let s = path.to_string_lossy().replace('/', "\\");
    let trimmed = s.trim_end_matches('\\');
    if trimmed.len() == 2 && trimmed.as_bytes().get(1) == Some(&b':') {
        format!("{}\\", trimmed.to_ascii_uppercase())
    } else {
        trimmed.to_ascii_uppercase()
    }
}

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

        let mut status_message = String::from("j/k:Move  Enter:Open  BS/-:Up  Tab:Tree  y/x/p:Yank/Cut/Paste  /:Search  ;:Actions  F10:Menu  q:Quit");
        if !rejected.is_empty() {
            status_message = format!("Skipped invalid keybindings: {}", rejected.join(", "));
        }

        let drives = win_drives::list_logical_drives();
        let mut app = Self {
            current_dir: initial_dir,
            entries: Vec::new(),
            visible_indices: Vec::new(),
            focused_vis_idx: 0,
            list_rows_per_col: 20,
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
            external_apps: default_external_apps(),
            clipboard: None,
            search_query: String::new(),
            search_regex: false,
            overlay: OverlayMode::None,
            status_message,
            should_quit: false,
            pending_launch: None,
        };
        app.expand_ancestors_of_current_dir();
        app.reload_dir();
        app.sync_tree_cursor_to_current_dir();
        app.record_visit();
        app
    }

    /// Updates `list_rows_per_col` from the current terminal dimensions so column navigation
    /// (`h`/`l`, `Left`/`Right`, `PageUp`/`PageDown`) matches the actual rendered column height.
    pub fn update_viewport_size(&mut self, _term_width: u16, term_height: u16) {
        let top_rows = 1 + if self.search_query.is_empty() { 1 } else { 2 };
        let status_rows = 1;
        let main_height = term_height.saturating_sub(top_rows + status_rows);
        // Subtract 2 for the top and bottom borders of the FileList block
        self.list_rows_per_col = main_height.saturating_sub(2).max(1) as usize;
    }

    pub fn reload_dir(&mut self) {
        match query::fs_list_dir(
            &self.current_dir,
            self.ui_state.ui_show_hidden,
            &self.ui_state.sort_key,
            &self.ui_state.sort_order,
        ) {
            Ok(list) => {
                self.entries = list;
                self.apply_filter();
            }
            Err(e) => {
                self.status_message = format!("Cannot read folder: {e}");
                self.entries.clear();
                self.visible_indices.clear();
                self.focused_vis_idx = 0;
            }
        }
        let cur_key = normalize_tree_key(&self.current_dir);
        self.tree_children_cache.remove(&cur_key);
        if let Some(parent) = self.current_dir.parent() {
            self.tree_children_cache.remove(&normalize_tree_key(parent));
        }
        self.rebuild_tree();
    }

    fn load_child_dirs(&mut self, dir: &Path) -> Vec<(String, PathBuf)> {
        let key = normalize_tree_key(dir);
        if let Some(cached) = self.tree_children_cache.get(&key) {
            return cached.clone();
        }
        let children: Vec<(String, PathBuf)> = query::fs_list_dir(
            dir,
            self.ui_state.ui_show_hidden,
            "name",
            "asc",
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.entry_type == EntryType::Dir)
        .map(|e| (e.name, e.path))
        .collect();
        self.tree_children_cache.insert(key, children.clone());
        children
    }

    pub fn expand_ancestors_of_current_dir(&mut self) {
        let mut ancestors: Vec<PathBuf> = self
            .current_dir
            .ancestors()
            .map(Path::to_path_buf)
            .collect();
        ancestors.reverse();
        // Expand ancestor chain up to current_dir's parent so current_dir is visible in the tree,
        // and also expand current_dir itself so its immediate child folders are shown.
        for anc in ancestors {
            if !anc.as_os_str().is_empty() {
                self.expanded_dirs.insert(normalize_tree_key(&anc));
            }
        }
    }

    pub fn rebuild_tree(&mut self) {
        let mut nodes = Vec::new();
        let drives = self.drives.clone();
        for d in drives {
            let root_path = PathBuf::from(format!("{d}\\"));
            let root_name = format!("{d}\\");
            self.append_tree_hierarchy(&root_path, &root_name, 0, &mut nodes);
        }
        // Fallback for tests or non-standard roots not under `self.drives`
        if nodes.is_empty() {
            if let Some(root) = self.current_dir.ancestors().last() {
                let root_path = root.to_path_buf();
                let root_name = root_path.to_string_lossy().into_owned();
                self.append_tree_hierarchy(&root_path, &root_name, 0, &mut nodes);
            }
        }
        self.tree_nodes = nodes;
        if self.tree_cursor >= self.tree_nodes.len() {
            self.tree_cursor = self.tree_nodes.len().saturating_sub(1);
        }
    }

    fn append_tree_hierarchy(
        &mut self,
        path: &Path,
        name: &str,
        depth: usize,
        out: &mut Vec<TreeNode>,
    ) {
        let key = normalize_tree_key(path);
        let expanded = self.expanded_dirs.contains(&key);
        out.push(TreeNode {
            path: path.to_path_buf(),
            name: name.to_string(),
            depth,
            expanded,
        });
        if expanded && depth < 32 {
            let mut children = self.load_child_dirs(path);
            // Ensure any ancestor of `current_dir` (e.g. hidden `AppData` or `.tmp` folder)
            // whose parent is `path` is included so the active directory path is always visible.
            for anc in self.current_dir.ancestors() {
                if let Some(parent) = anc.parent() {
                    if normalize_tree_key(parent) == key {
                        let anc_key = normalize_tree_key(anc);
                        if !children
                            .iter()
                            .any(|(_, p)| normalize_tree_key(p) == anc_key)
                        {
                            if let Some(os_name) = anc.file_name() {
                                children.push((
                                    os_name.to_string_lossy().into_owned(),
                                    anc.to_path_buf(),
                                ));
                                children.sort_by(|a, b| {
                                    a.0.to_lowercase().cmp(&b.0.to_lowercase())
                                });
                            }
                        }
                    }
                }
            }
            for (child_name, child_path) in children {
                self.append_tree_hierarchy(&child_path, &child_name, depth + 1, out);
            }
        }
    }

    pub fn sync_tree_cursor_to_current_dir(&mut self) {
        let target_key = normalize_tree_key(&self.current_dir);
        if let Some(idx) = self
            .tree_nodes
            .iter()
            .position(|n| normalize_tree_key(&n.path) == target_key)
        {
            self.tree_cursor = idx;
        }
    }

    pub fn apply_filter(&mut self) {
        if self.search_query.is_empty() {
            self.visible_indices = (0..self.entries.len()).collect();
        } else if self.search_regex {
            match RegexBuilder::new(&self.search_query).case_insensitive(true).build() {
                Ok(re) => {
                    self.visible_indices = self
                        .entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| re.is_match(&e.name))
                        .map(|(i, _)| i)
                        .collect();
                }
                Err(_) => {
                    self.visible_indices.clear();
                }
            }
        } else {
            let needle = self.search_query.to_lowercase();
            self.visible_indices = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.name.to_lowercase().contains(&needle))
                .map(|(i, _)| i)
                .collect();
        }
        if self.focused_vis_idx >= self.visible_indices.len() {
            self.focused_vis_idx = self.visible_indices.len().saturating_sub(1);
        }
    }

    pub fn focused_entry(&self) -> Option<&Entry> {
        let &real_idx = self.visible_indices.get(self.focused_vis_idx)?;
        self.entries.get(real_idx)
    }

    pub fn operation_targets(&self) -> Vec<PathBuf> {
        let visible_selected: Vec<PathBuf> = self
            .visible_indices
            .iter()
            .filter_map(|&idx| self.entries.get(idx))
            .filter(|e| self.selected_paths.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        if !visible_selected.is_empty() {
            visible_selected
        } else if let Some(entry) = self.focused_entry() {
            vec![entry.path.clone()]
        } else {
            Vec::new()
        }
    }

    pub fn navigate_to(&mut self, dir: PathBuf) {
        if !dir.is_dir() {
            self.status_message = format!("Folder does not exist: {}", dir.display());
            return;
        }
        let prev_dir = self.current_dir.clone();
        let new_dir_key = normalize_tree_key(&dir);
        let child_to_focus = prev_dir
            .ancestors()
            .find(|anc| {
                anc.parent()
                    .is_some_and(|p| normalize_tree_key(p) == new_dir_key)
            })
            .map(Path::to_path_buf);

        self.current_dir = dir;
        self.selected_paths.clear();
        self.focused_vis_idx = 0;
        self.search_query.clear();
        self.expand_ancestors_of_current_dir();
        self.reload_dir();
        if let Some(target_child) = child_to_focus {
            let target_key = normalize_tree_key(&target_child);
            if let Some(vis_idx) = self.visible_indices.iter().position(|&real_idx| {
                normalize_tree_key(&self.entries[real_idx].path) == target_key
            }) {
                self.focused_vis_idx = vis_idx;
            }
        }
        self.sync_tree_cursor_to_current_dir();
        self.record_visit();
    }

    fn record_visit(&mut self) {
        let p = self.current_dir.to_string_lossy().into_owned();
        self.nav_data.visit_history.retain(|x| !x.eq_ignore_ascii_case(&p));
        self.nav_data.visit_history.insert(0, p);
        if self.nav_data.visit_history.len() > 50 {
            self.nav_data.visit_history.truncate(50);
        }
        self.save_navigation();
    }

    fn save_navigation(&self) {
        if let Ok(path) = paths::navigation_path() {
            let _ = crate::config::navigation::save(&path, &self.nav_data);
        }
    }

    fn save_ui_state(&self) {
        if let Ok(path) = paths::ui_state_path() {
            let _ = crate::config::ui_state::save(&path, &self.ui_state);
        }
    }

    pub fn handle_key_event(&mut self, ev: KeyEvent) {
        if !matches!(self.overlay, OverlayMode::None) {
            self.handle_overlay_key(ev);
            return;
        }

        // Tab switches focus between Tree and FileList when tree is visible
        if ev.code == KeyCode::Tab && ev.modifiers.is_empty() {
            if self.ui_state.ui_show_tree {
                self.focus_pane = match self.focus_pane {
                    FocusPane::FileList => FocusPane::Tree,
                    FocusPane::Tree => FocusPane::FileList,
                };
            }
            return;
        }

        // Menu bar activation: F10 or Alt+F/E/V/G (without Ctrl)
        if ev.code == KeyCode::F(10) {
            self.overlay = OverlayMode::MenuBar { cat_idx: 0, item_idx: 0 };
            return;
        }
        if ev.modifiers == KeyModifiers::ALT {
            if let KeyCode::Char(c) = ev.code {
                let cat = match c.to_ascii_uppercase() {
                    'F' => Some(0),
                    'E' => Some(1),
                    'V' => Some(2),
                    'G' => Some(3),
                    _ => None,
                };
                if let Some(cat_idx) = cat {
                    self.overlay = OverlayMode::MenuBar { cat_idx, item_idx: 0 };
                    return;
                }
            }
        }

        let Some(chord) = KeyChord::from_crossterm(ev) else {
            return;
        };

        // Check registered external app hotkeys
        for idx in 0..self.external_apps.len() {
            let matched = self.external_apps[idx]
                .key
                .as_ref()
                .is_some_and(|chords| chords.contains(&chord));
            if matched {
                self.trigger_external_app(idx);
                return;
            }
        }

        if let Some(action) = self.keymap.resolve(&chord) {
            self.execute_action(action);
        }
    }

    pub fn execute_action(&mut self, action: ActionId) {
        match action {
            ActionId::MoveUp => {
                if self.focus_pane == FocusPane::Tree {
                    self.tree_cursor = self.tree_cursor.saturating_sub(1);
                } else {
                    self.focused_vis_idx = self.focused_vis_idx.saturating_sub(1);
                }
            }
            ActionId::MoveDown => {
                if self.focus_pane == FocusPane::Tree {
                    if !self.tree_nodes.is_empty() {
                        self.tree_cursor = (self.tree_cursor + 1).min(self.tree_nodes.len() - 1);
                    }
                } else if !self.visible_indices.is_empty() {
                    self.focused_vis_idx = (self.focused_vis_idx + 1).min(self.visible_indices.len() - 1);
                }
            }
            ActionId::MoveLeft => {
                if self.focus_pane == FocusPane::Tree {
                    if let Some(node) = self.tree_nodes.get(self.tree_cursor).cloned() {
                        let key = normalize_tree_key(&node.path);
                        if node.expanded {
                            self.expanded_dirs.remove(&key);
                            self.rebuild_tree();
                        } else if let Some(parent_idx) = (0..self.tree_cursor)
                            .rev()
                            .find(|&i| self.tree_nodes[i].depth < node.depth)
                        {
                            self.tree_cursor = parent_idx;
                        }
                    }
                } else {
                    let rows = self.list_rows_per_col.max(1);
                    if self.focused_vis_idx >= rows {
                        self.focused_vis_idx -= rows;
                    }
                }
            }
            ActionId::MoveRight => {
                if self.focus_pane == FocusPane::Tree {
                    if let Some(node) = self.tree_nodes.get(self.tree_cursor).cloned() {
                        let key = normalize_tree_key(&node.path);
                        if !node.expanded {
                            self.tree_children_cache.remove(&key);
                            self.expanded_dirs.insert(key);
                            self.rebuild_tree();
                        } else if let Some(next_node) = self.tree_nodes.get(self.tree_cursor + 1) {
                            if next_node.depth == node.depth + 1 {
                                self.tree_cursor += 1;
                            }
                        }
                    }
                } else if !self.visible_indices.is_empty() {
                    let rows = self.list_rows_per_col.max(1);
                    let current_col = self.focused_vis_idx / rows;
                    let total_cols = self.visible_indices.len().div_ceil(rows);
                    if current_col + 1 < total_cols {
                        self.focused_vis_idx =
                            (self.focused_vis_idx + rows).min(self.visible_indices.len() - 1);
                    }
                }
            }
            ActionId::MoveTop => {
                if self.focus_pane == FocusPane::Tree {
                    self.tree_cursor = 0;
                } else {
                    self.focused_vis_idx = 0;
                }
            }
            ActionId::MoveBottom => {
                if self.focus_pane == FocusPane::Tree {
                    if !self.tree_nodes.is_empty() {
                        self.tree_cursor = self.tree_nodes.len() - 1;
                    }
                } else if !self.visible_indices.is_empty() {
                    self.focused_vis_idx = self.visible_indices.len() - 1;
                }
            }
            ActionId::PageUp => {
                let rows = self.list_rows_per_col.max(1);
                if self.focus_pane == FocusPane::Tree {
                    self.tree_cursor = self.tree_cursor.saturating_sub(rows);
                } else {
                    self.focused_vis_idx = self.focused_vis_idx.saturating_sub(rows);
                }
            }
            ActionId::PageDown => {
                let rows = self.list_rows_per_col.max(1);
                if self.focus_pane == FocusPane::Tree {
                    if !self.tree_nodes.is_empty() {
                        self.tree_cursor = (self.tree_cursor + rows).min(self.tree_nodes.len() - 1);
                    }
                } else if !self.visible_indices.is_empty() {
                    self.focused_vis_idx = (self.focused_vis_idx + rows).min(self.visible_indices.len() - 1);
                }
            }
            ActionId::SelectToggle => {
                if self.focus_pane == FocusPane::Tree {
                    if let Some(node) = self.tree_nodes.get(self.tree_cursor).cloned() {
                        let key = normalize_tree_key(&node.path);
                        if node.expanded {
                            self.expanded_dirs.remove(&key);
                        } else {
                            self.tree_children_cache.remove(&key);
                            self.expanded_dirs.insert(key);
                        }
                        self.rebuild_tree();
                    }
                } else if let Some(entry) = self.focused_entry().cloned() {
                    if !self.selected_paths.remove(&entry.path) {
                        self.selected_paths.insert(entry.path);
                    }
                }
            }
            ActionId::SelectAll => {
                for &idx in &self.visible_indices {
                    if let Some(e) = self.entries.get(idx) {
                        self.selected_paths.insert(e.path.clone());
                    }
                }
            }
            ActionId::ClearSelection => {
                self.selected_paths.clear();
                if !self.search_query.is_empty() {
                    self.search_query.clear();
                    self.apply_filter();
                }
                self.status_message.clear();
            }
            ActionId::Open => {
                if self.focus_pane == FocusPane::Tree {
                    if let Some(node) = self.tree_nodes.get(self.tree_cursor).cloned() {
                        let target = node.path;
                        self.focus_pane = FocusPane::FileList;
                        self.navigate_to(target);
                    }
                    return;
                }
                if let Some(entry) = self.focused_entry().cloned() {
                    if entry.entry_type == EntryType::Dir {
                        self.navigate_to(entry.path);
                    } else {
                        self.open_in_viewer(&entry.path);
                    }
                }
            }
            ActionId::GoParent => {
                if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) {
                    if parent != self.current_dir {
                        self.navigate_to(parent);
                    }
                }
            }
            ActionId::Exit => {
                self.should_quit = true;
            }
            ActionId::FocusPath => {
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::PathBar,
                    title: "Enter Path (Enter:Go / Esc:Cancel)".to_string(),
                    buffer: self.current_dir.to_string_lossy().into_owned(),
                };
            }
            ActionId::ToggleHidden => {
                self.ui_state.ui_show_hidden = !self.ui_state.ui_show_hidden;
                self.save_ui_state();
                self.reload_dir();
                self.status_message = format!(
                    "Hidden files: {}",
                    if self.ui_state.ui_show_hidden { "ON" } else { "OFF" }
                );
            }
            ActionId::ToggleTree => {
                self.ui_state.ui_show_tree = !self.ui_state.ui_show_tree;
                if !self.ui_state.ui_show_tree {
                    self.focus_pane = FocusPane::FileList;
                }
                self.save_ui_state();
            }
            ActionId::ToggleSize => {
                self.ui_state.ui_show_size = !self.ui_state.ui_show_size;
                self.save_ui_state();
            }
            ActionId::ToggleTime => {
                self.ui_state.ui_show_time = !self.ui_state.ui_show_time;
                self.save_ui_state();
            }
            ActionId::Copy => {
                let targets = self.operation_targets();
                if !targets.is_empty() {
                    let count = targets.len();
                    self.clipboard = Some(FileClipboard {
                        mode: ClipMode::Copy,
                        items: targets,
                    });
                    self.status_message = format!("Yanked {count} item(s) (press p to paste)");
                }
            }
            ActionId::Cut => {
                let targets = self.operation_targets();
                if !targets.is_empty() {
                    let count = targets.len();
                    self.clipboard = Some(FileClipboard {
                        mode: ClipMode::Cut,
                        items: targets,
                    });
                    self.status_message = format!("Cut {count} item(s) (press p to move)");
                }
            }
            ActionId::Paste => {
                let Some(clip) = self.clipboard.clone() else {
                    self.status_message = "Clipboard is empty".to_string();
                    return;
                };
                let summary = match clip.mode {
                    ClipMode::Copy => transfer::copy_batch(
                        &clip.items,
                        &self.current_dir,
                        transfer::ConflictResolution::Overwrite,
                    ),
                    ClipMode::Cut => {
                        let s = transfer::move_batch(
                            &clip.items,
                            &self.current_dir,
                            transfer::ConflictResolution::Overwrite,
                        );
                        if s.failed == 0 {
                            self.clipboard = None;
                        }
                        s
                    }
                };
                self.reload_dir();
                self.status_message = format!(
                    "Paste complete: {} succeeded / {} failed",
                    summary.ok, summary.failed
                );
            }
            ActionId::Delete | ActionId::DeletePermanent => {
                let targets = self.operation_targets();
                if targets.is_empty() {
                    return;
                }
                let permanent = action == ActionId::DeletePermanent;
                let title = if permanent {
                    format!("Permanently delete {} item(s)? (y/Enter:Confirm / Esc:Cancel)", targets.len())
                } else {
                    format!("Move {} item(s) to Trash? (y/Enter:Confirm / Esc:Cancel)", targets.len())
                };
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::ConfirmDelete { permanent, targets },
                    title,
                    buffer: String::new(),
                };
            }
            ActionId::Rename => {
                if let Some(entry) = self.focused_entry().cloned() {
                    self.overlay = OverlayMode::Input {
                        kind: InputPromptKind::Rename { target: entry.path },
                        title: "Rename (Enter:Apply / Esc:Cancel)".to_string(),
                        buffer: entry.name,
                    };
                }
            }
            ActionId::NewFile => {
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::NewFile,
                    title: "New File Name (Enter:Create / Esc:Cancel)".to_string(),
                    buffer: String::new(),
                };
            }
            ActionId::NewFolder => {
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::NewFolder,
                    title: "New Folder Name (Enter:Create / Esc:Cancel)".to_string(),
                    buffer: String::new(),
                };
            }
            ActionId::Search => {
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::Search,
                    title: "Search Filter (Ctrl+R:Toggle Regex / Enter:Keep / Esc:Clear)".to_string(),
                    buffer: self.search_query.clone(),
                };
            }
            ActionId::SortMenu => {
                self.overlay = OverlayMode::SortMenu { cursor: 0 };
            }
            ActionId::History => {
                self.overlay = OverlayMode::ListPopup {
                    title: "Visit History (j/k:Select / Enter:Go / d:Remove / Esc:Close)".to_string(),
                    items: self.nav_data.visit_history.clone(),
                    cursor: 0,
                    is_jump_list: false,
                };
            }
            ActionId::HistoryJumpList => {
                self.overlay = OverlayMode::ListPopup {
                    title: "Jump List (j/k:Select / Enter:Go / d:Remove / Esc:Close)".to_string(),
                    items: self.nav_data.jump_list.clone(),
                    cursor: 0,
                    is_jump_list: true,
                };
            }
            ActionId::JumpAdd => {
                let p = self.current_dir.to_string_lossy().into_owned();
                self.nav_data.jump_list.retain(|x| !x.eq_ignore_ascii_case(&p));
                self.nav_data.jump_list.insert(0, p.clone());
                self.save_navigation();
                self.status_message = format!("Added to Jump List: {p}");
            }
            ActionId::ZipCreate => {
                let targets = self.operation_targets();
                if targets.is_empty() {
                    self.status_message = "No items selected to compress".to_string();
                    return;
                }
                let default_name = if targets.len() == 1 {
                    let stem = targets[0]
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("archive");
                    format!("{stem}.zip")
                } else {
                    "archive.zip".to_string()
                };
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::ZipCreate,
                    title: "Output ZIP File Name (Enter:Create / Esc:Cancel)".to_string(),
                    buffer: default_name,
                };
            }
            ActionId::ZipExtract => {
                let Some(entry) = self.focused_entry().cloned() else {
                    return;
                };
                if !entry.name.to_ascii_lowercase().ends_with(".zip") {
                    self.status_message = "Please select a .zip file".to_string();
                    return;
                }
                let stem = entry
                    .path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("extracted")
                    .to_string();
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::ZipExtract { zip_path: entry.path },
                    title: "Extract Destination Folder (Enter:Extract / Esc:Cancel)".to_string(),
                    buffer: stem,
                };
            }
            ActionId::ActionMenu => {
                self.overlay = OverlayMode::ActionMenu { cursor: 0 };
            }
        }
    }

    pub fn trigger_external_app(&mut self, index: usize) {
        let Some(app) = self.external_apps.get(index).cloned() else {
            return;
        };
        let target = if let Some(entry) = self.focused_entry() {
            LaunchTarget::from_entry(&entry.path, entry.entry_type == EntryType::Dir, &self.current_dir)
        } else {
            LaunchTarget::from_current_dir(&self.current_dir)
        };

        let Some(exe) = resolve_executable(&app.program) else {
            self.status_message = format!(
                "Cannot launch '{}': executable '{}' not found in PATH",
                app.name, app.program
            );
            return;
        };

        let args = match build_args(&app, &target) {
            Ok(a) => a,
            Err(e) => {
                self.status_message = format!("Argument error for '{}': {e}", app.name);
                return;
            }
        };

        self.pending_launch = Some(PendingExternalLaunch {
            app_name: app.name,
            exe,
            args,
            working_dir: target.dir,
        });
    }

    fn open_in_viewer(&mut self, path: &Path) {
        use std::io::Read;
        let ext = classify::extension_of(path);
        let mut sample = Vec::new();
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.take(65_536).read_to_end(&mut sample);
        }
        match classify::decide_kind(ext.as_deref(), &sample) {
            classify::ViewerKind::Text | classify::ViewerKind::Markdown => match text::read_text(path) {
                Ok(payload) => {
                    let lines = crate::viewer::highlight::highlight_with_bat(path, &payload.content);
                    let title = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Viewer")
                        .to_string();
                    self.overlay = OverlayMode::Viewer {
                        path: path.to_path_buf(),
                        title,
                        lines,
                        scroll_y: 0,
                        scroll_x: 0,
                    };
                }
                Err(e) => {
                    self.status_message = format!("Cannot open in viewer: {e}");
                }
            },
            classify::ViewerKind::Image => {
                self.status_message = "Image preview is not supported in TUI viewer".to_string();
            }
            classify::ViewerKind::OsOpen => {
                self.status_message = "Binary files cannot be displayed in the built-in viewer (use external tools like bat)".to_string();
            }
        }
    }

    fn handle_overlay_key(&mut self, ev: KeyEvent) {
        match self.overlay.clone() {
            OverlayMode::None => {}
            OverlayMode::Input {
                kind,
                title,
                mut buffer,
            } => {
                if let InputPromptKind::ConfirmDelete { permanent, targets } = kind {
                    match ev.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let summary = if permanent {
                                delete::delete_permanently(&targets)
                            } else {
                                delete::delete_to_trash(&targets)
                            };
                            self.overlay = OverlayMode::None;
                            self.selected_paths.clear();
                            self.reload_dir();
                            self.status_message = format!(
                                "Delete complete: {} succeeded / {} failed",
                                summary.ok, summary.failed
                            );
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            self.overlay = OverlayMode::None;
                        }
                        _ => {}
                    }
                    return;
                }

                match ev.code {
                    KeyCode::Esc => {
                        if kind == InputPromptKind::Search {
                            self.search_query.clear();
                            self.apply_filter();
                        }
                        self.overlay = OverlayMode::None;
                    }
                    KeyCode::Enter => {
                        self.overlay = OverlayMode::None;
                        self.commit_input_prompt(kind, &buffer);
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                        if kind == InputPromptKind::Search {
                            self.search_query = buffer.clone();
                            self.apply_filter();
                        }
                        self.overlay = OverlayMode::Input { kind, title, buffer };
                    }
                    KeyCode::Char('r') if ev.modifiers.contains(KeyModifiers::CONTROL) && kind == InputPromptKind::Search => {
                        self.search_regex = !self.search_regex;
                        self.apply_filter();
                        self.overlay = OverlayMode::Input { kind, title, buffer };
                    }
                    KeyCode::Char(c) if !ev.modifiers.contains(KeyModifiers::CONTROL) && !ev.modifiers.contains(KeyModifiers::ALT) => {
                        buffer.push(c);
                        if kind == InputPromptKind::Search {
                            self.search_query = buffer.clone();
                            self.apply_filter();
                        }
                        self.overlay = OverlayMode::Input { kind, title, buffer };
                    }
                    _ => {}
                }
            }
            OverlayMode::SortMenu { mut cursor } => {
                const SORT_OPTIONS: &[(&str, &str, &str)] = &[
                    ("name", "asc", "Name (Asc)"),
                    ("name", "desc", "Name (Desc)"),
                    ("size", "asc", "Size (Asc)"),
                    ("size", "desc", "Size (Desc)"),
                    ("type", "asc", "Type (Asc)"),
                    ("type", "desc", "Type (Desc)"),
                    ("modified", "asc", "Modified (Asc)"),
                    ("modified", "desc", "Modified (Desc)"),
                ];
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1).min(SORT_OPTIONS.len() - 1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    KeyCode::Enter => {
                        let (k, o, label) = SORT_OPTIONS[cursor];
                        self.ui_state.sort_key = k.to_string();
                        self.ui_state.sort_order = o.to_string();
                        self.save_ui_state();
                        self.overlay = OverlayMode::None;
                        self.reload_dir();
                        self.status_message = format!("Sorted by: {label}");
                    }
                    _ => {}
                }
            }
            OverlayMode::ListPopup {
                title,
                mut items,
                mut cursor,
                is_jump_list,
            } => match ev.code {
                KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    cursor = cursor.saturating_sub(1);
                    self.overlay = OverlayMode::ListPopup {
                        title,
                        items,
                        cursor,
                        is_jump_list,
                    };
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !items.is_empty() {
                        cursor = (cursor + 1).min(items.len() - 1);
                    }
                    self.overlay = OverlayMode::ListPopup {
                        title,
                        items,
                        cursor,
                        is_jump_list,
                    };
                }
                KeyCode::Delete | KeyCode::Char('d') => {
                    if cursor < items.len() {
                        items.remove(cursor);
                        if is_jump_list {
                            self.nav_data.jump_list = items.clone();
                        } else {
                            self.nav_data.visit_history = items.clone();
                        }
                        self.save_navigation();
                        if cursor >= items.len() && !items.is_empty() {
                            cursor = items.len() - 1;
                        }
                    }
                    self.overlay = OverlayMode::ListPopup {
                        title,
                        items,
                        cursor,
                        is_jump_list,
                    };
                }
                KeyCode::Enter => {
                    self.overlay = OverlayMode::None;
                    if let Some(target_str) = items.get(cursor) {
                        self.navigate_to(PathBuf::from(target_str));
                    }
                }
                _ => {}
            },
            OverlayMode::ActionMenu { mut cursor } => {
                let total = 10 + self.external_apps.len();
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1).min(total.saturating_sub(1));
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    KeyCode::Enter => {
                        self.overlay = OverlayMode::None;
                        match cursor {
                            0 => self.execute_action(ActionId::Open),
                            1 => self.execute_action(ActionId::Copy),
                            2 => self.execute_action(ActionId::Cut),
                            3 => self.execute_action(ActionId::Paste),
                            4 => self.execute_action(ActionId::Rename),
                            5 => self.execute_action(ActionId::Delete),
                            6 => self.execute_action(ActionId::NewFile),
                            7 => self.execute_action(ActionId::NewFolder),
                            8 => self.execute_action(ActionId::ZipCreate),
                            9 => self.execute_action(ActionId::ZipExtract),
                            ext_idx => self.trigger_external_app(ext_idx - 10),
                        }
                    }
                    _ => {}
                }
            }
            OverlayMode::MenuBar {
                mut cat_idx,
                mut item_idx,
            } => {
                let cat_counts = [8usize + self.external_apps.len(), 6, 5, 5];
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Left | KeyCode::Char('h') => {
                        cat_idx = if cat_idx == 0 { 3 } else { cat_idx - 1 };
                        item_idx = 0;
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        cat_idx = (cat_idx + 1) % 4;
                        item_idx = 0;
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        item_idx = item_idx.saturating_sub(1);
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        item_idx = (item_idx + 1).min(cat_counts[cat_idx].saturating_sub(1));
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Enter => {
                        self.overlay = OverlayMode::None;
                        self.execute_menubar_item(cat_idx, item_idx);
                    }
                    _ => {}
                }
            }
            OverlayMode::Viewer {
                path,
                title,
                lines,
                mut scroll_y,
                mut scroll_x,
            } => match ev.code {
                KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                KeyCode::Down | KeyCode::Char('j') => {
                    scroll_y = (scroll_y + 1).min(lines.len().saturating_sub(1));
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    scroll_y = scroll_y.saturating_sub(1);
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::PageDown | KeyCode::Char('d') if ev.code == KeyCode::PageDown || ev.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll_y = (scroll_y + 20).min(lines.len().saturating_sub(1));
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::PageUp | KeyCode::Char('u') if ev.code == KeyCode::PageUp || ev.modifiers.contains(KeyModifiers::CONTROL) => {
                    scroll_y = scroll_y.saturating_sub(20);
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::Home | KeyCode::Char('g') => {
                    scroll_y = 0;
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::End | KeyCode::Char('G') => {
                    scroll_y = lines.len().saturating_sub(1);
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    scroll_x = scroll_x.saturating_add(4);
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    scroll_x = scroll_x.saturating_sub(4);
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        lines,
                        scroll_y,
                        scroll_x,
                    };
                }
                _ => {}
            },
        }
    }

    fn execute_menubar_item(&mut self, cat_idx: usize, item_idx: usize) {
        match cat_idx {
            0 => {
                // File: Open, New File, New Folder, Rename, Delete, Create ZIP, Extract ZIP, [External CUI tools...], Exit
                let ext_count = self.external_apps.len();
                match item_idx {
                    0 => self.execute_action(ActionId::Open),
                    1 => self.execute_action(ActionId::NewFile),
                    2 => self.execute_action(ActionId::NewFolder),
                    3 => self.execute_action(ActionId::Rename),
                    4 => self.execute_action(ActionId::Delete),
                    5 => self.execute_action(ActionId::ZipCreate),
                    6 => self.execute_action(ActionId::ZipExtract),
                    i if i >= 7 && i < 7 + ext_count => self.trigger_external_app(i - 7),
                    _ => self.execute_action(ActionId::Exit),
                }
            }
            1 => match item_idx {
                0 => self.execute_action(ActionId::Cut),
                1 => self.execute_action(ActionId::Copy),
                2 => self.execute_action(ActionId::Paste),
                3 => self.execute_action(ActionId::SelectAll),
                4 => self.execute_action(ActionId::ClearSelection),
                _ => self.execute_action(ActionId::Search),
            },
            2 => match item_idx {
                0 => self.execute_action(ActionId::ToggleTree),
                1 => self.execute_action(ActionId::ToggleHidden),
                2 => self.execute_action(ActionId::ToggleSize),
                3 => self.execute_action(ActionId::ToggleTime),
                _ => self.execute_action(ActionId::SortMenu),
            },
            _ => match item_idx {
                0 => self.execute_action(ActionId::GoParent),
                1 => self.execute_action(ActionId::FocusPath),
                2 => self.execute_action(ActionId::History),
                3 => self.execute_action(ActionId::HistoryJumpList),
                _ => self.execute_action(ActionId::JumpAdd),
            },
        }
    }

    fn commit_input_prompt(&mut self, kind: InputPromptKind, value: &str) {
        let trimmed = value.trim();
        match kind {
            InputPromptKind::PathBar => {
                if !trimmed.is_empty() {
                    self.navigate_to(PathBuf::from(trimmed));
                }
            }
            InputPromptKind::Search => {
                self.search_query = value.to_string();
                self.apply_filter();
            }
            InputPromptKind::Rename { target } => {
                if !trimmed.is_empty() {
                    match rename::rename_entry(&target, trimmed) {
                        Ok(_) => {
                            self.reload_dir();
                            self.status_message = format!("Renamed to: {trimmed}");
                        }
                        Err(e) => {
                            self.status_message = format!("Rename failed: {e:?}");
                        }
                    }
                }
            }
            InputPromptKind::NewFile => {
                if !trimmed.is_empty() {
                    match create::create_entry(&self.current_dir, trimmed, "file") {
                        Ok(()) => {
                            self.reload_dir();
                            self.status_message = format!("Created file: {trimmed}");
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to create file: {e:?}");
                        }
                    }
                }
            }
            InputPromptKind::NewFolder => {
                if !trimmed.is_empty() {
                    match create::create_entry(&self.current_dir, trimmed, "dir") {
                        Ok(()) => {
                            self.reload_dir();
                            self.status_message = format!("Created folder: {trimmed}");
                        }
                        Err(e) => {
                            self.status_message = format!("Failed to create folder: {e:?}");
                        }
                    }
                }
            }
            InputPromptKind::ZipCreate => {
                let targets: Vec<String> = self
                    .operation_targets()
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                let base_dir = self.current_dir.to_string_lossy().into_owned();
                let hooks = crate::archive::create::CreateHooks::none();
                match crate::archive::guarded::create_guarded(&targets, trimmed, &base_dir, true, &hooks) {
                    Ok(res) => {
                        self.reload_dir();
                        self.status_message = format!("Created ZIP ({} entries): {}", res.entries, res.output_path.display());
                    }
                    Err(e) => {
                        self.status_message = format!("ZIP compression error: {}", e.message);
                    }
                }
            }
            InputPromptKind::ZipExtract { zip_path } => {
                let zip_str = zip_path.to_string_lossy().into_owned();
                let base_dir = self.current_dir.to_string_lossy().into_owned();
                let env = crate::archive::extract_plan::ExtractEnv::real();
                let hooks = crate::archive::extract::ExtractHooks::none();
                let resolutions = std::collections::HashMap::new();
                match crate::archive::guarded::extract_guarded(&zip_str, trimmed, &base_dir, &env, &resolutions, &hooks) {
                    Ok(res) => {
                        self.reload_dir();
                        self.status_message = format!("Extracted ZIP ({} files): {}", res.extracted, res.destination.display());
                    }
                    Err(e) => {
                        self.status_message = format!("ZIP extraction error: {}", e.message);
                    }
                }
            }
            InputPromptKind::ConfirmDelete { .. } => {}
        }
    }
}
