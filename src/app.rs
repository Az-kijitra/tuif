//! Application state and Vim-like controller for `cuifiler` (SPEC-0300-0001).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use regex::RegexBuilder;

use crate::config::paths;
use crate::external_apps::{
    build_args, default_external_apps, resolve_executable, ExternalAppEntry, FuzzyKind,
    FuzzySelection, LaunchTarget, PendingFuzzySearch,
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

/// Number of fixed items in the right-click ActionMenu (before the external app entries).
/// Order: Open, Open with Default App, Copy, Cut, Paste, Rename, Delete, New File, New Folder, Create ZIP, Extract ZIP.
pub const ACTION_MENU_FIXED_COUNT: usize = 11;

/// Snapshot of the file transfer progress for display in the TUI modal.
#[derive(Debug, Clone)]
pub struct TransferProgressState {
    pub title: String,
    pub current_file: String,
    pub current_file_bytes: u64,
    pub current_file_total: u64,
    pub total_bytes_transferred: u64,
    pub total_bytes: u64,
    pub files_transferred: usize,
    pub total_files: usize,
    pub is_cancelling: bool,
}

/// Data for an active conflict confirmation prompt.
pub struct ConflictPromptData {
    pub file_name: String,
    pub reply_tx: mpsc::Sender<ConflictChoice>,
    pub cursor: usize,
}

/// Messages sent from the background transfer thread to the main event loop.
pub enum TransferMessage {
    Progress(TransferProgressState),
    ConflictPrompt {
        file_name: String,
        reply_tx: mpsc::Sender<ConflictChoice>,
    },
    Finished {
        succeeded: usize,
        failed: usize,
        cancelled: bool,
        error_msg: Option<String>,
        is_cut: bool,
    },
}

/// Represents an active background transfer operation.
pub struct ActiveTransfer {
    pub cancel_flag: Arc<AtomicBool>,
    pub rx: mpsc::Receiver<TransferMessage>,
    pub current_state: TransferProgressState,
    pub conflict_prompt: Option<ConflictPromptData>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConflictPolicy {
    Ask,
    OverwriteAll,
    SkipAll,
}

fn resolve_conflict(
    policy: &std::sync::Mutex<ConflictPolicy>,
    file_name: &str,
    tx: &mpsc::Sender<TransferMessage>,
) -> ConflictChoice {
    let pol = policy.lock().unwrap();
    match *pol {
        ConflictPolicy::OverwriteAll => ConflictChoice::Overwrite,
        ConflictPolicy::SkipAll => ConflictChoice::Skip,
        ConflictPolicy::Ask => {
            let (reply_tx, reply_rx) = mpsc::channel();
            if tx
                .send(TransferMessage::ConflictPrompt {
                    file_name: file_name.to_string(),
                    reply_tx,
                })
                .is_err()
            {
                return ConflictChoice::Cancel;
            }
            drop(pol);
            match reply_rx.recv() {
                Ok(ConflictChoice::Overwrite) => ConflictChoice::Overwrite,
                Ok(ConflictChoice::Skip) => ConflictChoice::Skip,
                Ok(ConflictChoice::OverwriteAll) => {
                    *policy.lock().unwrap() = ConflictPolicy::OverwriteAll;
                    ConflictChoice::Overwrite
                }
                Ok(ConflictChoice::SkipAll) => {
                    *policy.lock().unwrap() = ConflictPolicy::SkipAll;
                    ConflictChoice::Skip
                }
                Ok(ConflictChoice::Cancel) | Err(_) => ConflictChoice::Cancel,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickTarget {
    FileItem(usize),
    TreeItem(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    FileList,
    Tree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteNodeInfo {
    ProfileRoot(String), // profile_id
    Directory { profile_id: String, path: String },
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
    pub remote: Option<RemoteNodeInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipMode {
    Copy,
    Cut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetDestination {
    Local(PathBuf),
    Remote { profile_id: String, path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardItem {
    Local(PathBuf),
    Remote { profile_id: String, path: String },
}

impl ClipboardItem {
    pub fn name(&self) -> String {
        match self {
            ClipboardItem::Local(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            ClipboardItem::Remote { path, .. } => rpath::file_name(path),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileClipboard {
    pub mode: ClipMode,
    pub items: Vec<ClipboardItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPromptKind {
    PathBar,
    Search,
    Rename { target: PathBuf },
    RenameRemote { target: String },
    NewFile,
    NewFolder,
    ZipCreate,
    ZipExtract { zip_path: PathBuf },
    ConfirmDelete { permanent: bool, targets: Vec<PathBuf> },
    ConfirmDeleteRemote { targets: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayMode {
    None,
    Input {
        kind: InputPromptKind,
        title: String,
        buffer: String,
    },
    PasswordPrompt {
        profile_id: String,
        title: String,
        buffer: String,
        save_password: bool,
    },
    HostKeyConfirm {
        endpoint: String,
        fingerprint: String,
    },
    ConnectionManager {
        cursor: usize,
    },
    ConnectionEdit {
        is_new: bool,
        profile: ConnectionProfile,
        cursor: usize,
        password_buffer: String,
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
        is_markdown: bool,
        preview_mode: bool,
        raw_lines: Vec<String>,
        preview_lines: Option<Vec<String>>,
        scroll_y: usize,
        scroll_x: usize,
        search_query: Option<String>,
        search_input: Option<String>,
        matching_lines: Vec<usize>,
        match_cursor: Option<usize>,
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
    pub term_width: u16,
    pub term_height: u16,
    pub last_click: Option<(Instant, ClickTarget)>,
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
    pub should_cd_on_exit: bool,
    pub pending_launch: Option<PendingExternalLaunch>,
    pub pending_fuzzy: Option<PendingFuzzySearch>,
    pub remote_profiles: Vec<ConnectionProfile>,
    pub known_hosts: KnownHosts,
    pub remote_session: Option<Arc<RemoteSession>>,
    pub remote_dir: Option<String>,
    pub pending_handshake: Option<PendingHandshake>,
    pub active_transfer: Option<ActiveTransfer>,
    pub theme: crate::theme::Theme,
    pub general: crate::config::config_toml::GeneralConfig,
    pub layout: crate::config::config_toml::LayoutConfig,
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

pub fn tree_node_key(node: &TreeNode) -> String {
    match &node.remote {
        None => normalize_tree_key(&node.path),
        Some(RemoteNodeInfo::ProfileRoot(id)) => format!("remote-root:{id}"),
        Some(RemoteNodeInfo::Directory { profile_id, path }) => {
            format!("remote:{profile_id}:{path}")
        }
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
        let theme = cfg_load.theme;
        let general = cfg_load.general;
        let layout = cfg_load.layout;

        let mut status_message = String::from("j/k:Move  Enter:Open  BS/-:Up  Tab:Tree  y/x/p:Yank/Cut/Paste  /:Search  ;:Actions  F10:Menu  q:Quit");
        if !rejected.is_empty() {
            status_message = format!("Skipped invalid keybindings: {}", rejected.join(", "));
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
            external_apps: default_external_apps(),
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

    pub fn reload_dir(&mut self) {
        if let (Some(session), Some(rdir)) = (&self.remote_session, &self.remote_dir) {
            let rdir = rdir.clone();
            match session.list_dir(&rdir, self.ui_state.ui_show_hidden) {
                Ok(mut list) => {
                    query::sort_entries_with_options(
                        &mut list,
                        &self.ui_state.sort_key,
                        &self.ui_state.sort_order,
                        self.general.dirs_first,
                        self.general.natural_sort,
                    );
                    self.entries = list;
                    self.apply_filter();
                }
                Err(e) => {
                    self.status_message = format!("Remote read error: {e}");
                    self.entries.clear();
                    self.visible_indices.clear();
                    self.focused_vis_idx = 0;
                }
            }
        } else {
            if !self.current_dir.exists() {
                if let Some(existing) = self
                    .current_dir
                    .ancestors()
                    .find(|p| p.is_dir())
                    .map(Path::to_path_buf)
                {
                    self.current_dir = existing;
                }
            }
            match query::fs_list_dir_with_options(
                &self.current_dir,
                self.ui_state.ui_show_hidden,
                &self.ui_state.sort_key,
                &self.ui_state.sort_order,
                self.general.dirs_first,
                self.general.natural_sort,
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
        }
        self.tree_children_cache.clear();
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

    fn load_remote_child_dirs(&mut self, profile_id: &str, dir: &str) -> Vec<(String, PathBuf)> {
        let key = format!("remote:{profile_id}:{dir}");
        if let Some(cached) = self.tree_children_cache.get(&key) {
            return cached.clone();
        }
        let mut children = Vec::new();
        if let Some(session) = &self.remote_session {
            if session.profile.id == profile_id {
                if let Ok(dirs) = session.list_child_dirs(dir, self.ui_state.ui_show_hidden) {
                    children = dirs
                        .into_iter()
                        .map(|(n, p)| (n, PathBuf::from(p)))
                        .collect();
                }
            }
        }
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

        // SFTP Remote Profiles
        let remote_profiles = self.remote_profiles.clone();
        for profile in &remote_profiles {
            let root_key = format!("remote-root:{}", profile.id);
            let expanded = self.expanded_dirs.contains(&root_key);
            let root_name = format!("☁ [SFTP] {}", profile.display_label());
            nodes.push(TreeNode {
                path: PathBuf::from(profile.initial_dir.as_str()),
                name: root_name,
                depth: 0,
                expanded,
                remote: Some(RemoteNodeInfo::ProfileRoot(profile.id.clone())),
            });
            if expanded {
                let start_dir = if let Some(session) = &self.remote_session {
                    if session.profile.id == profile.id {
                        Some(session.initial_dir())
                    } else {
                        None
                    }
                } else if !profile.initial_dir.is_empty() {
                    Some(profile.initial_dir.clone())
                } else {
                    Some("/".to_string())
                };

                if let Some(sdir) = start_dir {
                    self.append_remote_tree_hierarchy(&profile.id, &sdir, 1, &mut nodes);
                }
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
            remote: None,
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

    fn append_remote_tree_hierarchy(
        &mut self,
        profile_id: &str,
        path: &str,
        depth: usize,
        out: &mut Vec<TreeNode>,
    ) {
        let key = format!("remote:{profile_id}:{path}");
        let expanded = self.expanded_dirs.contains(&key);
        let display_name = if path == "/" {
            "/".to_string()
        } else {
            rpath::file_name(path)
        };
        out.push(TreeNode {
            path: PathBuf::from(path),
            name: display_name,
            depth,
            expanded,
            remote: Some(RemoteNodeInfo::Directory {
                profile_id: profile_id.to_string(),
                path: path.to_string(),
            }),
        });
        if expanded && depth < 32 {
            let children = self.load_remote_child_dirs(profile_id, path);
            for (_child_name, child_path) in children {
                let child_str = child_path.to_string_lossy();
                self.append_remote_tree_hierarchy(profile_id, &child_str, depth + 1, out);
            }
        }
    }

    pub fn sync_tree_cursor_to_current_dir(&mut self) {
        if let (Some(session), Some(rdir)) = (&self.remote_session, &self.remote_dir) {
            let target_key = format!("remote:{}:{rdir}", session.profile.id);
            if let Some(idx) = self.tree_nodes.iter().position(|n| tree_node_key(n) == target_key) {
                self.tree_cursor = idx;
            }
        } else {
            let target_key = normalize_tree_key(&self.current_dir);
            if let Some(idx) = self
                .tree_nodes
                .iter()
                .position(|n| tree_node_key(n) == target_key)
            {
                self.tree_cursor = idx;
            }
        }
    }

    pub fn navigate_to_remote(&mut self, profile_id: &str, dir: &str) {
        let rdir = rpath::normalize(dir);
        let is_connected = self
            .remote_session
            .as_ref()
            .map(|s| s.profile.id == profile_id)
            .unwrap_or(false);
        if !is_connected {
            self.connect_profile(profile_id, Some(rdir));
            return;
        }

        self.remote_dir = Some(rdir.clone());
        self.selected_paths.clear();
        self.focused_vis_idx = 0;
        self.search_query.clear();

        for anc in rpath::ancestors_root_first(&rdir) {
            self.expanded_dirs
                .insert(format!("remote:{profile_id}:{anc}"));
        }
        self.expanded_dirs
            .insert(format!("remote-root:{profile_id}"));

        self.reload_dir();
        self.sync_tree_cursor_to_current_dir();
    }

    pub fn connect_profile(&mut self, profile_id: &str, target_dir: Option<String>) {
        let Some(profile) = self
            .remote_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        else {
            self.status_message = format!("Connection profile not found: {profile_id}");
            return;
        };
        self.status_message = format!("Connecting to {}...", profile.display_label());

        match handshake(&profile, &self.known_hosts) {
            Ok(pending) => match pending.status {
                HostKeyStatus::Unknown => {
                    self.overlay = OverlayMode::HostKeyConfirm {
                        endpoint: profile.endpoint_key(),
                        fingerprint: pending.fingerprint.clone(),
                    };
                    self.pending_handshake = Some(pending);
                }
                HostKeyStatus::Mismatch { expected } => {
                    self.status_message = format!(
                        "HOST KEY MISMATCH for {}! Expected {expected}, got {}",
                        profile.endpoint_key(),
                        pending.fingerprint
                    );
                }
                HostKeyStatus::Trusted => {
                    self.finish_connect(pending, target_dir);
                }
            },
            Err(e) => {
                self.status_message = format!("Connection failed: {e}");
            }
        }
    }

    pub fn finish_connect(&mut self, pending: PendingHandshake, target_dir: Option<String>) {
        let profile = pending.profile.clone();
        let saved_pw = if profile.save_password {
            credential::load_password(&profile.credential_target())
        } else {
            None
        };

        if let Some(pw) = saved_pw {
            match pending.authenticate(&pw) {
                Ok(session) => {
                    let initial = target_dir.unwrap_or_else(|| session.initial_dir());
                    self.status_message = format!("Connected to {}", profile.display_label());
                    self.expanded_dirs
                        .insert(format!("remote-root:{}", profile.id));
                    self.remote_session = Some(Arc::new(session));
                    self.navigate_to_remote(&profile.id, &initial);
                }
                Err(e) => {
                    self.status_message = format!("Authentication error: {e}");
                    self.pending_handshake = None;
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id: profile.id.clone(),
                        title: format!("Password for {}", profile.display_label()),
                        buffer: String::new(),
                        save_password: profile.save_password,
                    };
                }
            }
        } else {
            self.pending_handshake = Some(pending);
            self.overlay = OverlayMode::PasswordPrompt {
                profile_id: profile.id.clone(),
                title: format!("Password for {}", profile.display_label()),
                buffer: String::new(),
                save_password: profile.save_password,
            };
        }
    }

    pub fn authenticate_and_connect(&mut self, password: &str, save_pw: bool) {
        let Some(pending) = self.pending_handshake.take() else {
            self.status_message = "No pending connection".to_string();
            return;
        };
        let profile = pending.profile.clone();
        match pending.authenticate(password) {
            Ok(session) => {
                if save_pw {
                    let _ = credential::save_password(&profile.credential_target(), &profile.user, password);
                }
                if let Some(p) = self.remote_profiles.iter_mut().find(|p| p.id == profile.id) {
                    p.save_password = save_pw;
                }
                self.save_remote_profiles();
                let initial = session.initial_dir();
                self.status_message = format!("Connected to {}", profile.display_label());
                self.expanded_dirs
                    .insert(format!("remote-root:{}", profile.id));
                self.remote_session = Some(Arc::new(session));
                self.navigate_to_remote(&profile.id, &initial);
            }
            Err(e) => {
                self.status_message = format!("Authentication error: {e}");
            }
        }
    }

    pub fn open_remote_in_viewer(&mut self, remote_path: &str) {
        let Some(session) = &self.remote_session else {
            return;
        };
        let temp_dir = std::env::temp_dir()
            .join("tuif-remote")
            .join(&session.profile.id);
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_name = rpath::file_name(remote_path);
        let local_temp = temp_dir.join(crate::remote::session::sanitize_windows_name(&file_name));
        self.status_message = format!("Downloading {file_name} for viewing...");
        match session.download(remote_path, &local_temp) {
            Ok(_) => {
                let url_title = format!(
                    "{}/{}",
                    session.profile.url_prefix(),
                    remote_path.trim_start_matches('/')
                );
                self.open_in_viewer(&local_temp);
                if let OverlayMode::Viewer { ref mut title, .. } = self.overlay {
                    *title = url_title;
                }
            }
            Err(e) => {
                self.status_message = format!("Download failed: {e}");
            }
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

    pub fn active_target_dir(&self) -> PathBuf {
        if self.focus_pane == FocusPane::Tree {
            if let Some(node) = self.tree_nodes.get(self.tree_cursor) {
                return node.path.clone();
            }
        }
        self.current_dir.clone()
    }

    pub fn operation_targets(&self) -> Vec<PathBuf> {
        if self.focus_pane == FocusPane::Tree {
            return self
                .tree_nodes
                .get(self.tree_cursor)
                .map(|n| vec![n.path.clone()])
                .unwrap_or_default();
        }
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

    pub fn target_destination(&self) -> TargetDestination {
        if self.focus_pane == FocusPane::Tree {
            if let Some(node) = self.tree_nodes.get(self.tree_cursor) {
                if let Some(remote) = &node.remote {
                    return match remote {
                        RemoteNodeInfo::Directory { profile_id, path } => {
                            TargetDestination::Remote {
                                profile_id: profile_id.clone(),
                                path: path.clone(),
                            }
                        }
                        RemoteNodeInfo::ProfileRoot(pid) => {
                            let profile = self.remote_profiles.iter().find(|p| &p.id == pid);
                            let initial = profile
                                .map(|p| p.initial_dir.clone())
                                .unwrap_or_else(|| "/".to_string());
                            TargetDestination::Remote {
                                profile_id: pid.clone(),
                                path: if initial.is_empty() {
                                    "/".to_string()
                                } else {
                                    initial
                                },
                            }
                        }
                    };
                }
                return TargetDestination::Local(node.path.clone());
            }
        }
        if let (Some(session), Some(rdir)) = (&self.remote_session, &self.remote_dir) {
            TargetDestination::Remote {
                profile_id: session.profile.id.clone(),
                path: rdir.clone(),
            }
        } else {
            TargetDestination::Local(self.current_dir.clone())
        }
    }

    pub fn operation_targets_clipboard(&self) -> Vec<ClipboardItem> {
        if self.focus_pane == FocusPane::Tree {
            if let Some(node) = self.tree_nodes.get(self.tree_cursor) {
                if let Some(remote) = &node.remote {
                    return match remote {
                        RemoteNodeInfo::Directory { profile_id, path } => {
                            vec![ClipboardItem::Remote {
                                profile_id: profile_id.clone(),
                                path: path.clone(),
                            }]
                        }
                        RemoteNodeInfo::ProfileRoot(_) => Vec::new(),
                    };
                }
                return vec![ClipboardItem::Local(node.path.clone())];
            }
            return Vec::new();
        }

        if let (Some(session), true) = (&self.remote_session, self.is_remote()) {
            let pid = session.profile.id.clone();
            let visible_selected: Vec<ClipboardItem> = self
                .visible_indices
                .iter()
                .filter_map(|&idx| self.entries.get(idx))
                .filter(|e| self.selected_paths.contains(&e.path))
                .map(|e| ClipboardItem::Remote {
                    profile_id: pid.clone(),
                    path: e.path.to_string_lossy().into_owned(),
                })
                .collect();
            if !visible_selected.is_empty() {
                visible_selected
            } else if let Some(entry) = self.focused_entry() {
                vec![ClipboardItem::Remote {
                    profile_id: pid,
                    path: entry.path.to_string_lossy().into_owned(),
                }]
            } else {
                Vec::new()
            }
        } else {
            self.operation_targets()
                .into_iter()
                .map(ClipboardItem::Local)
                .collect()
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
        self.remote_dir = None;
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

    pub fn navigate_to_and_focus_entry(&mut self, target_path: &Path) {
        let parent_dir = target_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.current_dir.clone());
        self.navigate_to(parent_dir);
        self.focus_pane = FocusPane::FileList;

        let target_key = normalize_tree_key(target_path);
        let find_idx = |app: &Self| -> Option<usize> {
            app.visible_indices
                .iter()
                .position(|&real_idx| normalize_tree_key(&app.entries[real_idx].path) == target_key)
                .or_else(|| {
                    let name_str = target_path.file_name()?.to_string_lossy();
                    app.visible_indices.iter().position(|&real_idx| {
                        app.entries[real_idx].name.eq_ignore_ascii_case(&name_str)
                    })
                })
        };

        if let Some(vis_idx) = find_idx(self) {
            self.focused_vis_idx = vis_idx;
        } else if !self.ui_state.ui_show_hidden && target_path.exists() {
            // If the target file is hidden and hidden files were off, enable show_hidden so we can focus it
            self.ui_state.ui_show_hidden = true;
            self.save_ui_state();
            self.reload_dir();
            if let Some(vis_idx) = find_idx(self) {
                self.focused_vis_idx = vis_idx;
            }
        }
    }

    pub fn apply_fuzzy_selection(&mut self, sel: FuzzySelection) {
        if sel.kind == FuzzyKind::JumpDir || sel.target_path.is_dir() {
            self.navigate_to(sel.target_path.clone());
            self.focus_pane = FocusPane::FileList;
            self.status_message = format!("Jumped to folder: {}", sel.target_path.display());
        } else {
            self.navigate_to_and_focus_entry(&sel.target_path);
            if let Some(line) = sel.line_number {
                self.status_message =
                    format!("Selected: {} (line {line})", sel.target_path.display());
            } else {
                self.status_message = format!("Selected: {}", sel.target_path.display());
            }
        }
    }

    pub fn trigger_fuzzy_search(&mut self, kind: FuzzyKind) {
        match self.target_destination() {
            TargetDestination::Local(working_dir) => {
                self.pending_fuzzy = Some(PendingFuzzySearch {
                    kind,
                    working_dir,
                    show_hidden: self.ui_state.ui_show_hidden,
                });
            }
            TargetDestination::Remote { .. } => {
                self.status_message =
                    "Fuzzy search (fzf / rg) is only available for local folders".to_string();
            }
        }
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
        if let Some(transfer) = self.active_transfer.as_mut() {
            if let Some(prompt) = transfer.conflict_prompt.as_mut() {
                let choice = match ev.code {
                    KeyCode::Left | KeyCode::BackTab => {
                        prompt.cursor = if prompt.cursor == 0 { 4 } else { prompt.cursor - 1 };
                        None
                    }
                    KeyCode::Right | KeyCode::Tab => {
                        prompt.cursor = (prompt.cursor + 1) % 5;
                        None
                    }
                    KeyCode::Char('o') | KeyCode::Char('O') => Some(ConflictChoice::Overwrite),
                    KeyCode::Char('s') | KeyCode::Char('S') => Some(ConflictChoice::Skip),
                    KeyCode::Char('a') | KeyCode::Char('A') => Some(ConflictChoice::OverwriteAll),
                    KeyCode::Char('k') | KeyCode::Char('K') => Some(ConflictChoice::SkipAll),
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => Some(ConflictChoice::Cancel),
                    KeyCode::Enter => match prompt.cursor {
                        0 => Some(ConflictChoice::Overwrite),
                        1 => Some(ConflictChoice::Skip),
                        2 => Some(ConflictChoice::OverwriteAll),
                        3 => Some(ConflictChoice::SkipAll),
                        _ => Some(ConflictChoice::Cancel),
                    },
                    _ => None,
                };
                if let Some(chosen) = choice {
                    if chosen == ConflictChoice::Cancel {
                        transfer.cancel_flag.store(true, Ordering::Relaxed);
                        transfer.current_state.is_cancelling = true;
                        self.status_message = "Cancelling transfer...".to_string();
                    }
                    let _ = prompt.reply_tx.send(chosen);
                    transfer.conflict_prompt = None;
                }
                return;
            }

            if ev.code == KeyCode::Esc {
                transfer.cancel_flag.store(true, Ordering::Relaxed);
                transfer.current_state.is_cancelling = true;
                self.status_message = "Cancelling transfer...".to_string();
            }
            return;
        }

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
                        let key = tree_node_key(&node);
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
                        let key = tree_node_key(&node);
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
                        let key = tree_node_key(&node);
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
                        if let Some(remote) = &node.remote {
                            match remote {
                                RemoteNodeInfo::ProfileRoot(pid) => {
                                    self.connect_profile(pid, None);
                                }
                                RemoteNodeInfo::Directory { profile_id, path } => {
                                    self.focus_pane = FocusPane::FileList;
                                    self.navigate_to_remote(profile_id, path);
                                }
                            }
                        } else {
                            let target = node.path;
                            self.focus_pane = FocusPane::FileList;
                            self.navigate_to(target);
                        }
                    }
                    return;
                }
                if self.is_remote() {
                    if let Some(entry) = self.focused_entry().cloned() {
                        let rpath_str = entry.path.to_string_lossy().into_owned();
                        if entry.entry_type == EntryType::Dir {
                            if let Some(session) = &self.remote_session {
                                let pid = session.profile.id.clone();
                                self.navigate_to_remote(&pid, &rpath_str);
                            }
                        } else {
                            self.open_remote_in_viewer(&rpath_str);
                        }
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
                if let (Some(session), Some(rdir)) = (&self.remote_session, &self.remote_dir) {
                    if let Some(parent) = rpath::parent(rdir) {
                        let pid = session.profile.id.clone();
                        self.navigate_to_remote(&pid, &parent);
                    }
                } else if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) {
                    if parent != self.current_dir {
                        self.navigate_to(parent);
                    }
                }
            }
            ActionId::Exit => {
                self.should_quit = true;
                self.should_cd_on_exit = false;
            }
            ActionId::ExitAndCd => {
                self.should_quit = true;
                self.should_cd_on_exit = true;
            }
            ActionId::FocusPath => {
                let buf = self.active_display_path();
                self.overlay = OverlayMode::Input {
                    kind: InputPromptKind::PathBar,
                    title: "Enter Path (Enter:Go / Esc:Cancel)".to_string(),
                    buffer: buf,
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
                let targets = self.operation_targets_clipboard();
                if !targets.is_empty() {
                    let count = targets.len();
                    let local_paths: Vec<PathBuf> = targets
                        .iter()
                        .filter_map(|t| match t {
                            ClipboardItem::Local(p) => Some(p.clone()),
                            ClipboardItem::Remote { .. } => None,
                        })
                        .collect();
                    if !local_paths.is_empty() {
                        crate::fs_ops::os_clipboard::set_os_clipboard_files(
                            &local_paths,
                            crate::fs_ops::os_clipboard::ClipboardEffect::Copy,
                        );
                    }
                    self.clipboard = Some(FileClipboard {
                        mode: ClipMode::Copy,
                        items: targets,
                    });
                    self.status_message = format!("Yanked {count} item(s) (press p to paste)");
                }
            }
            ActionId::Cut => {
                let targets = self.operation_targets_clipboard();
                if !targets.is_empty() {
                    let count = targets.len();
                    let local_paths: Vec<PathBuf> = targets
                        .iter()
                        .filter_map(|t| match t {
                            ClipboardItem::Local(p) => Some(p.clone()),
                            ClipboardItem::Remote { .. } => None,
                        })
                        .collect();
                    if !local_paths.is_empty() {
                        crate::fs_ops::os_clipboard::set_os_clipboard_files(
                            &local_paths,
                            crate::fs_ops::os_clipboard::ClipboardEffect::Move,
                        );
                    }
                    self.clipboard = Some(FileClipboard {
                        mode: ClipMode::Cut,
                        items: targets,
                    });
                    self.status_message = format!("Cut {count} item(s) (press p to move)");
                }
            }
            ActionId::Paste => {
                let os_clip = crate::fs_ops::os_clipboard::get_os_clipboard_files();
                let clip_to_paste = if let Some(os_payload) = os_clip {
                    let mode = match os_payload.effect {
                        crate::fs_ops::os_clipboard::ClipboardEffect::Copy => ClipMode::Copy,
                        crate::fs_ops::os_clipboard::ClipboardEffect::Move => ClipMode::Cut,
                    };
                    let items = os_payload
                        .paths
                        .into_iter()
                        .map(ClipboardItem::Local)
                        .collect();
                    Some(FileClipboard { mode, items })
                } else {
                    self.clipboard.clone()
                };

                let Some(clip) = clip_to_paste else {
                    self.status_message = "Clipboard is empty".to_string();
                    return;
                };
                self.execute_paste(clip);
            }
            ActionId::Delete | ActionId::DeletePermanent => {
                let targets = self.operation_targets_clipboard();
                if targets.is_empty() {
                    return;
                }
                let mut remote_targets = Vec::new();
                let mut local_targets = Vec::new();
                for t in targets {
                    match t {
                        ClipboardItem::Local(p) => local_targets.push(p),
                        ClipboardItem::Remote { path, .. } => remote_targets.push(path),
                    }
                }

                if !self.general.confirm_delete {
                    if !remote_targets.is_empty() {
                        if let Some(session) = &self.remote_session {
                            let mut succeeded = 0;
                            let mut failed = 0;
                            for t in &remote_targets {
                                match session.remove_recursive(t) {
                                    Ok(()) => succeeded += 1,
                                    Err(_) => failed += 1,
                                }
                            }
                            self.selected_paths.clear();
                            self.reload_dir();
                            self.status_message = format!(
                                "Remote delete complete: {succeeded} succeeded / {failed} failed"
                            );
                        }
                    } else if !local_targets.is_empty() {
                        let permanent = action == ActionId::DeletePermanent || !self.general.delete_to_trash;
                        let summary = if permanent {
                            delete::delete_permanently(&local_targets)
                        } else {
                            delete::delete_to_trash(&local_targets)
                        };
                        self.selected_paths.clear();
                        self.reload_dir();
                        self.status_message = format!(
                            "Delete complete: {} succeeded / {} failed",
                            summary.ok, summary.failed
                        );
                    }
                    return;
                }

                if !remote_targets.is_empty() {
                    let title = format!(
                        "Permanently delete {} remote item(s)? (y/Enter:Confirm / Esc:Cancel)",
                        remote_targets.len()
                    );
                    self.overlay = OverlayMode::Input {
                        kind: InputPromptKind::ConfirmDeleteRemote {
                            targets: remote_targets,
                        },
                        title,
                        buffer: String::new(),
                    };
                } else if !local_targets.is_empty() {
                    let permanent = action == ActionId::DeletePermanent || !self.general.delete_to_trash;
                    let title = if permanent {
                        format!(
                            "Permanently delete {} item(s)? (y/Enter:Confirm / Esc:Cancel)",
                            local_targets.len()
                        )
                    } else {
                        format!(
                            "Move {} item(s) to Trash? (y/Enter:Confirm / Esc:Cancel)",
                            local_targets.len()
                        )
                    };
                    self.overlay = OverlayMode::Input {
                        kind: InputPromptKind::ConfirmDelete {
                            permanent,
                            targets: local_targets,
                        },
                        title,
                        buffer: String::new(),
                    };
                }
            }
            ActionId::Rename => {
                if self.focus_pane == FocusPane::Tree {
                    if let Some(node) = self.tree_nodes.get(self.tree_cursor).cloned() {
                        if let Some(RemoteNodeInfo::Directory { path, .. }) = node.remote {
                            self.overlay = OverlayMode::Input {
                                kind: InputPromptKind::RenameRemote { target: path },
                                title: "Rename Remote Folder (Enter:Apply / Esc:Cancel)".to_string(),
                                buffer: node.name,
                            };
                        } else if node.remote.is_none() {
                            self.overlay = OverlayMode::Input {
                                kind: InputPromptKind::Rename { target: node.path },
                                title: "Rename (Enter:Apply / Esc:Cancel)".to_string(),
                                buffer: node.name,
                            };
                        }
                    }
                } else if self.is_remote() {
                    if let Some(entry) = self.focused_entry().cloned() {
                        let rpath = entry.path.to_string_lossy().into_owned();
                        self.overlay = OverlayMode::Input {
                            kind: InputPromptKind::RenameRemote { target: rpath },
                            title: "Rename Remote (Enter:Apply / Esc:Cancel)".to_string(),
                            buffer: entry.name,
                        };
                    }
                } else if let Some(entry) = self.focused_entry().cloned() {
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
                let p = self.active_target_dir().to_string_lossy().into_owned();
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
                if self.focus_pane == FocusPane::Tree {
                    self.status_message = "Please select a .zip file in Files".to_string();
                    return;
                }
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
            ActionId::ConnectionManager => {
                self.overlay = OverlayMode::ConnectionManager { cursor: 0 };
            }
            ActionId::FuzzyFindFile => {
                self.trigger_fuzzy_search(FuzzyKind::FindFile);
            }
            ActionId::FuzzyLiveGrep => {
                self.trigger_fuzzy_search(FuzzyKind::LiveGrep);
            }
            ActionId::FuzzyJumpDir => {
                self.trigger_fuzzy_search(FuzzyKind::JumpDir);
            }
            ActionId::OpenDefault => {
                self.open_focused_with_default_app();
            }
        }
    }

    pub fn poll_active_transfer(&mut self) {
        let Some(transfer) = self.active_transfer.as_mut() else {
            return;
        };

        let mut finished_msg = None;
        loop {
            match transfer.rx.try_recv() {
                Ok(TransferMessage::Progress(state)) => {
                    transfer.current_state = state;
                }
                Ok(TransferMessage::ConflictPrompt { file_name, reply_tx }) => {
                    transfer.conflict_prompt = Some(ConflictPromptData {
                        file_name,
                        reply_tx,
                        cursor: 0,
                    });
                    break;
                }
                Ok(TransferMessage::Finished {
                    succeeded,
                    failed,
                    cancelled,
                    error_msg,
                    is_cut,
                }) => {
                    finished_msg = Some((succeeded, failed, cancelled, error_msg, is_cut));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.active_transfer = None;
                    self.status_message = "Transfer aborted (worker disconnected)".to_string();
                    self.reload_dir();
                    return;
                }
            }
        }

        if let Some((succeeded, failed, cancelled, error_msg, is_cut)) = finished_msg {
            self.active_transfer = None;
            if cancelled {
                self.status_message = format!("Transfer cancelled ({succeeded} completed)");
            } else if let Some(err) = error_msg {
                self.status_message = format!("Transfer error: {err}");
            } else {
                self.status_message = format!(
                    "Transfer complete: {succeeded} succeeded / {failed} failed"
                );
            }
            if is_cut && !cancelled && failed == 0 {
                self.clipboard = None;
                crate::fs_ops::os_clipboard::clear_os_clipboard();
            }
            self.reload_dir();
        }
    }

    fn execute_paste(&mut self, clip: FileClipboard) {
        if self.active_transfer.is_some() {
            self.status_message = "Another transfer is currently in progress".to_string();
            return;
        }

        let dest = self.target_destination();
        match dest {
            TargetDestination::Local(dest_dir) => {
                let mut local_items = Vec::new();
                let mut remote_downloads = Vec::new();
                for item in &clip.items {
                    match item {
                        ClipboardItem::Local(p) => local_items.push(p.clone()),
                        ClipboardItem::Remote { profile_id, path } => {
                            remote_downloads.push((profile_id.clone(), path.clone()));
                        }
                    }
                }

                // If only local items exist, copy/move them in background with conflict resolution
                if remote_downloads.is_empty() {
                    if local_items.is_empty() {
                        return;
                    }
                    let cancel_flag = Arc::new(AtomicBool::new(false));
                    let (tx, rx) = mpsc::channel();
                    let is_cut = clip.mode == ClipMode::Cut;
                    let cancel_clone = Arc::clone(&cancel_flag);
                    let dest_clone = dest_dir.clone();
                    let items_clone = local_items.clone();

                    std::thread::spawn(move || {
                        let mut total_files = 0;
                        let mut total_bytes = 0;
                        for p in &items_clone {
                            if cancel_clone.load(Ordering::Relaxed) {
                                let _ = tx.send(TransferMessage::Finished {
                                    succeeded: 0,
                                    failed: 0,
                                    cancelled: true,
                                    error_msg: None,
                                    is_cut,
                                });
                                return;
                            }
                            let (f_count, b_count) = session::calc_local_size(p);
                            total_files += f_count;
                            total_bytes += b_count;
                        }

                        let _ = tx.send(TransferMessage::Progress(TransferProgressState {
                            title: if is_cut { "Moving files...".to_string() } else { "Copying files...".to_string() },
                            current_file: String::new(),
                            current_file_bytes: 0,
                            current_file_total: 0,
                            total_bytes_transferred: 0,
                            total_bytes,
                            files_transferred: 0,
                            total_files,
                            is_cancelling: false,
                        }));

                        let policy = std::sync::Mutex::new(ConflictPolicy::Ask);
                        let tx_clone = tx.clone();
                        let cancel_for_resolver = Arc::clone(&cancel_clone);
                        let dest_for_resolver = dest_clone.clone();

                        let resolver = move |item: &Path| {
                            let fname = item
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let target = dest_for_resolver.join(&fname);
                            if !target.exists() {
                                return transfer::ConflictResolution::Overwrite;
                            }
                            let choice = resolve_conflict(&policy, &fname, &tx_clone);
                            match choice {
                                ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {
                                    transfer::ConflictResolution::Overwrite
                                }
                                ConflictChoice::Skip | ConflictChoice::SkipAll => {
                                    transfer::ConflictResolution::Skip
                                }
                                ConflictChoice::Cancel => {
                                    cancel_for_resolver.store(true, Ordering::Relaxed);
                                    transfer::ConflictResolution::Reject
                                }
                            }
                        };

                        let cancel_check = || cancel_clone.load(Ordering::Relaxed);
                        let tx_for_event = tx.clone();
                        let title_str = if is_cut { "Moving files...".to_string() } else { "Copying files...".to_string() };
                        let on_event = move |ev: transfer::ProgressEvent| {
                            if ev.kind == transfer::ProgressEventKind::Start {
                                let _ = tx_for_event.send(TransferMessage::Progress(TransferProgressState {
                                    title: title_str.clone(),
                                    current_file: ev.name,
                                    current_file_bytes: 0,
                                    current_file_total: 0,
                                    total_bytes_transferred: 0,
                                    total_bytes,
                                    files_transferred: ev.index.saturating_sub(1) as usize,
                                    total_files,
                                    is_cancelling: cancel_check(),
                                }));
                            }
                        };

                        let summary = match clip.mode {
                            ClipMode::Copy => transfer::copy_batch_with_resolver(
                                &items_clone,
                                &dest_clone,
                                resolver,
                                cancel_check,
                                on_event,
                            ),
                            ClipMode::Cut => transfer::move_batch_with_resolver(
                                &items_clone,
                                &dest_clone,
                                resolver,
                                cancel_check,
                                on_event,
                            ),
                        };

                        let cancelled = cancel_clone.load(Ordering::Relaxed);
                        let _ = tx.send(TransferMessage::Finished {
                            succeeded: summary.ok as usize,
                            failed: summary.failed as usize,
                            cancelled,
                            error_msg: None,
                            is_cut,
                        });
                    });

                    self.active_transfer = Some(ActiveTransfer {
                        cancel_flag,
                        rx,
                        current_state: TransferProgressState {
                            title: if clip.mode == ClipMode::Cut { "Moving files...".to_string() } else { "Copying files...".to_string() },
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
                    return;
                }

                // Remote downloads: spawn background worker with progress channel
                let Some(session) = self
                    .remote_session
                    .as_ref()
                    .filter(|s| remote_downloads.iter().all(|(pid, _)| &s.profile.id == pid))
                    .cloned()
                else {
                    self.status_message = "Active remote session does not match download items".to_string();
                    return;
                };

                let cancel_flag = Arc::new(AtomicBool::new(false));
                let (tx, rx) = mpsc::channel();
                let is_cut = clip.mode == ClipMode::Cut;
                let cancel_clone = Arc::clone(&cancel_flag);
                let dest_clone = dest_dir.clone();

                std::thread::spawn(move || {
                    // Pre-scan total files and bytes
                    let mut total_files = 0;
                    let mut total_bytes = 0;
                    for (_, rpath) in &remote_downloads {
                        if cancel_clone.load(Ordering::Relaxed) {
                            let _ = tx.send(TransferMessage::Finished {
                                succeeded: 0,
                                failed: 0,
                                cancelled: true,
                                error_msg: None,
                                is_cut,
                            });
                            return;
                        }
                        if let Ok((f_count, b_count)) = session.calc_remote_size(rpath, &cancel_clone) {
                            total_files += f_count;
                            total_bytes += b_count;
                        }
                    }

                    let _ = tx.send(TransferMessage::Progress(TransferProgressState {
                        title: "Downloading from Remote...".to_string(),
                        current_file: String::new(),
                        current_file_bytes: 0,
                        current_file_total: 0,
                        total_bytes_transferred: 0,
                        total_bytes,
                        files_transferred: 0,
                        total_files,
                        is_cancelling: false,
                    }));

                    let mut total_transferred = 0u64;
                    let mut total_completed_files = 0usize;
                    let mut succeeded = 0;
                    let mut failed = 0;
                    let mut cancelled = false;
                    let policy = std::sync::Mutex::new(ConflictPolicy::Ask);

                    for (_, rpath) in &remote_downloads {
                        if cancel_clone.load(Ordering::Relaxed) {
                            cancelled = true;
                            break;
                        }
                        let fname = rpath::file_name(rpath);
                        let local_target = dest_clone.join(session::sanitize_windows_name(&fname));
                        let tx_clone = tx.clone();
                        let cancel_ref = &cancel_clone;
                        let base_bytes = total_transferred;
                        let base_files = total_completed_files;

                        let res = session.download_with_progress(
                            rpath,
                            &local_target,
                            cancel_ref,
                            |p| {
                                let _ = tx_clone.send(TransferMessage::Progress(TransferProgressState {
                                    title: "Downloading from Remote...".to_string(),
                                    current_file: p.file_name.clone(),
                                    current_file_bytes: p.file_bytes_transferred,
                                    current_file_total: p.file_total_bytes,
                                    total_bytes_transferred: base_bytes + p.total_bytes_transferred,
                                    total_bytes,
                                    files_transferred: base_files + p.completed_files,
                                    total_files,
                                    is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                }));
                            },
                            |f| resolve_conflict(&policy, f, &tx_clone),
                        );

                        match res {
                            Ok(stats) => {
                                succeeded += 1;
                                total_completed_files += stats.files as usize;
                                total_transferred += stats.bytes;
                                if is_cut {
                                    let _ = session.remove_recursive(rpath);
                                }
                            }
                            Err(_) => {
                                if cancel_ref.load(Ordering::Relaxed) {
                                    cancelled = true;
                                    break;
                                }
                                failed += 1;
                            }
                        }
                    }

                    let _ = tx.send(TransferMessage::Finished {
                        succeeded,
                        failed,
                        cancelled,
                        error_msg: None,
                        is_cut,
                    });
                });

                self.active_transfer = Some(ActiveTransfer {
                    cancel_flag,
                    rx,
                    current_state: TransferProgressState {
                        title: "Starting Download...".to_string(),
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
            }
            TargetDestination::Remote {
                profile_id,
                path: dest_rdir,
            } => {
                let Some(session) = self
                    .remote_session
                    .as_ref()
                    .filter(|s| s.profile.id == profile_id)
                    .cloned()
                else {
                    self.status_message =
                        "Remote session is not active for destination".to_string();
                    return;
                };

                // Check if this is a simple intra-remote Cut (rename)
                let is_pure_remote_cut = clip.mode == ClipMode::Cut
                    && clip.items.iter().all(|item| match item {
                        ClipboardItem::Remote { profile_id: p, .. } => p == &profile_id,
                        _ => false,
                    });

                if is_pure_remote_cut {
                    let mut succeeded = 0;
                    let mut failed = 0;
                    for item in &clip.items {
                        if let ClipboardItem::Remote { path: src_path, .. } = item {
                            let fname = rpath::file_name(src_path);
                            let rtarget = rpath::join(&dest_rdir, &fname);
                            match session.rename(src_path, &rtarget) {
                                Ok(()) => succeeded += 1,
                                Err(_) => failed += 1,
                            }
                        }
                    }
                    if failed == 0 {
                        self.clipboard = None;
                    }
                    self.reload_dir();
                    self.status_message = format!(
                        "Remote paste complete: {succeeded} succeeded / {failed} failed"
                    );
                    return;
                }

                // Otherwise, perform background transfer (upload or remote-to-remote copy)
                let cancel_flag = Arc::new(AtomicBool::new(false));
                let (tx, rx) = mpsc::channel();
                let is_cut = clip.mode == ClipMode::Cut;
                let cancel_clone = Arc::clone(&cancel_flag);
                let items = clip.items.clone();
                let target_dir = dest_rdir.clone();

                std::thread::spawn(move || {
                    // Pre-scan total files and bytes
                    let mut total_files = 0;
                    let mut total_bytes = 0;
                    for item in &items {
                        if cancel_clone.load(Ordering::Relaxed) {
                            let _ = tx.send(TransferMessage::Finished {
                                succeeded: 0,
                                failed: 0,
                                cancelled: true,
                                error_msg: None,
                                is_cut,
                            });
                            return;
                        }
                        match item {
                            ClipboardItem::Local(local_path) => {
                                let (f_count, b_count) = session::calc_local_size(local_path);
                                total_files += f_count;
                                total_bytes += b_count;
                            }
                            ClipboardItem::Remote { path: src_path, .. } => {
                                if let Ok((f_count, b_count)) = session.calc_remote_size(src_path, &cancel_clone) {
                                    total_files += f_count;
                                    total_bytes += b_count;
                                }
                            }
                        }
                    }

                    let _ = tx.send(TransferMessage::Progress(TransferProgressState {
                        title: "Uploading to Remote...".to_string(),
                        current_file: String::new(),
                        current_file_bytes: 0,
                        current_file_total: 0,
                        total_bytes_transferred: 0,
                        total_bytes,
                        files_transferred: 0,
                        total_files,
                        is_cancelling: false,
                    }));

                    let mut total_transferred = 0u64;
                    let mut total_completed_files = 0usize;
                    let mut succeeded = 0;
                    let mut failed = 0;
                    let mut cancelled = false;
                    let policy = std::sync::Mutex::new(ConflictPolicy::Ask);

                    for item in &items {
                        if cancel_clone.load(Ordering::Relaxed) {
                            cancelled = true;
                            break;
                        }

                        match item {
                            ClipboardItem::Local(local_path) => {
                                let fname = local_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| "unnamed".to_string());
                                let rtarget = rpath::join(&target_dir, &fname);
                                let tx_clone = tx.clone();
                                let cancel_ref = &cancel_clone;
                                let base_bytes = total_transferred;
                                let base_files = total_completed_files;

                                let res = session.upload_with_progress(
                                    local_path,
                                    &rtarget,
                                    cancel_ref,
                                    |p| {
                                        let _ = tx_clone.send(TransferMessage::Progress(TransferProgressState {
                                            title: "Uploading to Remote...".to_string(),
                                            current_file: p.file_name.clone(),
                                            current_file_bytes: p.file_bytes_transferred,
                                            current_file_total: p.file_total_bytes,
                                            total_bytes_transferred: base_bytes + p.total_bytes_transferred,
                                            total_bytes,
                                            files_transferred: base_files + p.completed_files,
                                            total_files,
                                            is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                        }));
                                    },
                                    |f| resolve_conflict(&policy, f, &tx_clone),
                                );

                                match res {
                                    Ok(stats) => {
                                        succeeded += 1;
                                        total_completed_files += stats.files as usize;
                                        total_transferred += stats.bytes;
                                        if is_cut {
                                            let _ = if local_path.is_dir() {
                                                std::fs::remove_dir_all(local_path)
                                            } else {
                                                std::fs::remove_file(local_path)
                                            };
                                        }
                                    }
                                    Err(_) => {
                                        if cancel_ref.load(Ordering::Relaxed) {
                                            cancelled = true;
                                            break;
                                        }
                                        failed += 1;
                                    }
                                }
                            }
                            ClipboardItem::Remote {
                                profile_id: src_pid,
                                path: src_path,
                            } => {
                                if src_pid == &session.profile.id {
                                    let fname = rpath::file_name(src_path);
                                    let rtarget = rpath::join(&target_dir, &fname);
                                    let temp_dir = std::env::temp_dir().join("tuif-copy");
                                    let _ = std::fs::create_dir_all(&temp_dir);
                                    let temp_path = temp_dir.join(session::sanitize_windows_name(&fname));
                                    let cancel_ref = &cancel_clone;
                                    let tx_clone = tx.clone();
                                    let base_bytes = total_transferred;
                                    let base_files = total_completed_files;

                                    let dl_res = session.download_with_progress(
                                        src_path,
                                        &temp_path,
                                        cancel_ref,
                                        |p| {
                                            let _ = tx_clone.send(TransferMessage::Progress(TransferProgressState {
                                                title: "Copying Remote File (Downloading)...".to_string(),
                                                current_file: p.file_name.clone(),
                                                current_file_bytes: p.file_bytes_transferred,
                                                current_file_total: p.file_total_bytes,
                                                total_bytes_transferred: base_bytes + (p.total_bytes_transferred / 2),
                                                total_bytes,
                                                files_transferred: base_files + (p.completed_files / 2),
                                                total_files,
                                                is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                            }));
                                        },
                                        |_| ConflictChoice::Overwrite,
                                    );

                                    if dl_res.is_ok() && !cancel_ref.load(Ordering::Relaxed) {
                                        let ul_res = session.upload_with_progress(
                                            &temp_path,
                                            &rtarget,
                                            cancel_ref,
                                            |p| {
                                                let _ = tx_clone.send(TransferMessage::Progress(TransferProgressState {
                                                    title: "Copying Remote File (Uploading)...".to_string(),
                                                    current_file: p.file_name.clone(),
                                                    current_file_bytes: p.file_bytes_transferred,
                                                    current_file_total: p.file_total_bytes,
                                                    total_bytes_transferred: base_bytes + (p.total_bytes_transferred / 2),
                                                    total_bytes,
                                                    files_transferred: base_files + p.completed_files,
                                                    total_files,
                                                    is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                                }));
                                            },
                                            |f| resolve_conflict(&policy, f, &tx_clone),
                                        );
                                        let _ = std::fs::remove_file(&temp_path);
                                        if let Ok(stats) = ul_res {
                                            succeeded += 1;
                                            total_completed_files += stats.files as usize;
                                            total_transferred += stats.bytes;
                                        } else {
                                            failed += 1;
                                        }
                                    } else {
                                        let _ = std::fs::remove_file(&temp_path);
                                        if cancel_ref.load(Ordering::Relaxed) {
                                            cancelled = true;
                                            break;
                                        }
                                        failed += 1;
                                    }
                                } else {
                                    failed += 1;
                                }
                            }
                        }
                    }

                    let _ = tx.send(TransferMessage::Finished {
                        succeeded,
                        failed,
                        cancelled,
                        error_msg: None,
                        is_cut,
                    });
                });

                self.active_transfer = Some(ActiveTransfer {
                    cancel_flag,
                    rx,
                    current_state: TransferProgressState {
                        title: "Starting Upload...".to_string(),
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
            }
        }
    }

    pub fn trigger_external_app(&mut self, index: usize) {
        let Some(app) = self.external_apps.get(index).cloned() else {
            return;
        };
        let target = if self.focus_pane == FocusPane::Tree {
            if let Some(node) = self.tree_nodes.get(self.tree_cursor) {
                LaunchTarget::from_entry(&node.path, true, &self.current_dir)
            } else {
                LaunchTarget::from_current_dir(&self.current_dir)
            }
        } else if let Some(entry) = self.focused_entry() {
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

        if app.is_gui {
            match crate::external_apps::spawn_gui_nonblocking(&exe, &args, &target.dir) {
                Ok(()) => {
                    self.status_message = format!("Launched {}", app.name);
                }
                Err(e) => {
                    self.status_message = e;
                }
            }
            return;
        }

        self.pending_launch = Some(PendingExternalLaunch {
            app_name: app.name,
            exe,
            args,
            working_dir: target.dir,
        });
    }

    /// Opens the focused item (or Tree node) with the Windows default associated application.
    /// Remote files are downloaded to a temp folder first; remote folders are not supported.
    pub fn open_focused_with_default_app(&mut self) {
        let local_target: PathBuf = if self.focus_pane == FocusPane::Tree {
            let Some(node) = self.tree_nodes.get(self.tree_cursor) else {
                return;
            };
            if node.remote.is_some() {
                self.status_message =
                    "Open with default app is not available for remote folders".to_string();
                return;
            }
            node.path.clone()
        } else if self.is_remote() {
            let Some(entry) = self.focused_entry().cloned() else {
                return;
            };
            if entry.entry_type == EntryType::Dir {
                self.status_message =
                    "Open with default app is not available for remote folders".to_string();
                return;
            }
            let Some(session) = &self.remote_session else {
                return;
            };
            let remote_path = entry.path.to_string_lossy().into_owned();
            let temp_dir = std::env::temp_dir()
                .join("tuif-remote")
                .join(&session.profile.id);
            let _ = std::fs::create_dir_all(&temp_dir);
            let file_name = rpath::file_name(&remote_path);
            let local_temp =
                temp_dir.join(crate::remote::session::sanitize_windows_name(&file_name));
            if let Err(e) = session.download(&remote_path, &local_temp) {
                self.status_message = format!("Download failed: {e}");
                return;
            }
            local_temp
        } else if let Some(entry) = self.focused_entry() {
            entry.path.clone()
        } else {
            return;
        };

        let name = local_target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| local_target.to_string_lossy().into_owned());
        match crate::external_apps::open_with_default_app(&local_target) {
            Ok(()) => self.status_message = format!("Opened with default app: {name}"),
            Err(e) => self.status_message = e,
        }
    }

    fn open_in_viewer(&mut self, path: &Path) {
        use std::io::Read;
        let ext = classify::extension_of(path);
        let mut sample = Vec::new();
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.take(65_536).read_to_end(&mut sample);
        }
        let kind = classify::decide_kind(ext.as_deref(), &sample);
        match kind {
            classify::ViewerKind::Text | classify::ViewerKind::Markdown => match text::read_text(path) {
                Ok(payload) => {
                    let is_markdown = kind == classify::ViewerKind::Markdown;
                    let raw_lines = crate::viewer::highlight::highlight_with_bat(path, &payload.content);
                    let preview_lines = if is_markdown {
                        let width = self.term_width.saturating_sub(6).max(40) as usize;
                        crate::viewer::highlight::render_markdown_with_leaf(&payload.content, width)
                    } else {
                        None
                    };
                    let preview_mode = preview_lines.is_some();
                    let title = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Viewer")
                        .to_string();
                    self.overlay = OverlayMode::Viewer {
                        path: path.to_path_buf(),
                        title,
                        is_markdown,
                        preview_mode,
                        raw_lines,
                        preview_lines,
                        scroll_y: 0,
                        scroll_x: 0,
                        search_query: None,
                        search_input: None,
                        matching_lines: Vec::new(),
                        match_cursor: None,
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
                            let permanent = permanent || !self.general.delete_to_trash;
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
                if let InputPromptKind::ConfirmDeleteRemote { targets } = kind {
                    match ev.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            self.overlay = OverlayMode::None;
                            if let Some(session) = &self.remote_session {
                                let mut succeeded = 0;
                                let mut failed = 0;
                                for t in &targets {
                                    match session.remove_recursive(t) {
                                        Ok(()) => succeeded += 1,
                                        Err(_) => failed += 1,
                                    }
                                }
                                self.selected_paths.clear();
                                self.reload_dir();
                                self.status_message = format!(
                                    "Remote delete complete: {succeeded} succeeded / {failed} failed"
                                );
                            }
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
                let total = ACTION_MENU_FIXED_COUNT + self.external_apps.len();
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
                        self.execute_action_menu_item(cursor);
                    }
                    _ => {}
                }
            }
            OverlayMode::MenuBar {
                mut cat_idx,
                mut item_idx,
            } => {
                let cat_counts = [10usize + self.external_apps.len(), 6, 5, 9];
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
                is_markdown,
                mut preview_mode,
                raw_lines,
                preview_lines,
                mut scroll_y,
                mut scroll_x,
                mut search_query,
                mut search_input,
                mut matching_lines,
                mut match_cursor,
            } => {
                let active_lines = if preview_mode {
                    preview_lines.as_ref().unwrap_or(&raw_lines)
                } else {
                    &raw_lines
                };
                let active_len = active_lines.len();

                // If currently typing into the search prompt
                if let Some(mut input) = search_input {
                    match ev.code {
                        KeyCode::Esc => {
                            search_input = None;
                        }
                        KeyCode::Enter => {
                            let query = input.trim().to_string();
                            search_input = None;
                            if query.is_empty() {
                                search_query = None;
                                matching_lines.clear();
                                match_cursor = None;
                            } else {
                                matching_lines = crate::viewer::highlight::find_matching_lines(active_lines, &query);
                                search_query = Some(query.clone());
                                if matching_lines.is_empty() {
                                    match_cursor = None;
                                    self.status_message = format!("Pattern not found: {query}");
                                } else {
                                    let next_pos = matching_lines.iter().position(|&l| l >= scroll_y).unwrap_or(0);
                                    scroll_y = matching_lines[next_pos];
                                    match_cursor = Some(next_pos);
                                }
                            }
                        }
                        KeyCode::Backspace => {
                            input.pop();
                            search_input = Some(input);
                        }
                        KeyCode::Char(c) => {
                            input.push(c);
                            search_input = Some(input);
                        }
                        _ => {
                            search_input = Some(input);
                        }
                    }
                    self.overlay = OverlayMode::Viewer {
                        path,
                        title,
                        is_markdown,
                        preview_mode,
                        raw_lines,
                        preview_lines,
                        scroll_y,
                        scroll_x,
                        search_query,
                        search_input,
                        matching_lines,
                        match_cursor,
                    };
                    return;
                }

                match ev.code {
                    KeyCode::Esc => {
                        if search_query.is_some() {
                            search_query = None;
                            matching_lines.clear();
                            match_cursor = None;
                            self.overlay = OverlayMode::Viewer {
                                path,
                                title,
                                is_markdown,
                                preview_mode,
                                raw_lines,
                                preview_lines,
                                scroll_y,
                                scroll_x,
                                search_query,
                                search_input,
                                matching_lines,
                                match_cursor,
                            };
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Char('/') | KeyCode::Char('f') if ev.code == KeyCode::Char('/') || ev.modifiers.contains(KeyModifiers::CONTROL) => {
                        search_input = Some(search_query.clone().unwrap_or_default());
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Char('n') => {
                        if !matching_lines.is_empty() {
                            let cur = match_cursor.unwrap_or(0);
                            let next = (cur + 1) % matching_lines.len();
                            scroll_y = matching_lines[next];
                            match_cursor = Some(next);
                        }
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Char('N') => {
                        if !matching_lines.is_empty() {
                            let cur = match_cursor.unwrap_or(0);
                            let prev = if cur == 0 { matching_lines.len() - 1 } else { cur - 1 };
                            scroll_y = matching_lines[prev];
                            match_cursor = Some(prev);
                        }
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Tab | KeyCode::Char('p') | KeyCode::Char('m') if is_markdown => {
                        if let Some(ref prev) = preview_lines {
                            preview_mode = !preview_mode;
                            let new_len = if preview_mode { prev.len() } else { raw_lines.len() };
                            scroll_y = scroll_y.min(new_len.saturating_sub(1));
                            scroll_x = 0;
                            let new_active = if preview_mode { prev.as_slice() } else { raw_lines.as_slice() };
                            if let Some(ref q) = search_query {
                                matching_lines = crate::viewer::highlight::find_matching_lines(new_active, q);
                                match_cursor = if matching_lines.is_empty() {
                                    None
                                } else {
                                    Some(matching_lines.iter().position(|&l| l >= scroll_y).unwrap_or(0))
                                };
                            }
                        } else {
                            self.status_message = "leaf is not available for Markdown preview".to_string();
                        }
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        scroll_y = (scroll_y + 1).min(active_len.saturating_sub(1));
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        scroll_y = scroll_y.saturating_sub(1);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::PageDown | KeyCode::Char('d') if ev.code == KeyCode::PageDown || ev.modifiers.contains(KeyModifiers::CONTROL) => {
                        scroll_y = (scroll_y + 20).min(active_len.saturating_sub(1));
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::PageUp | KeyCode::Char('u') if ev.code == KeyCode::PageUp || ev.modifiers.contains(KeyModifiers::CONTROL) => {
                        scroll_y = scroll_y.saturating_sub(20);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Home | KeyCode::Char('g') => {
                        scroll_y = 0;
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::End | KeyCode::Char('G') => {
                        scroll_y = active_len.saturating_sub(1);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        scroll_x = scroll_x.saturating_add(4);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        scroll_x = scroll_x.saturating_sub(4);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    _ => {}
                }
            }
            OverlayMode::PasswordPrompt {
                profile_id,
                title,
                mut buffer,
                mut save_password,
            } => match ev.code {
                KeyCode::Esc => {
                    self.overlay = OverlayMode::None;
                    self.pending_handshake = None;
                    self.status_message = "Connection cancelled".to_string();
                }
                KeyCode::Tab => {
                    save_password = !save_password;
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id,
                        title,
                        buffer,
                        save_password,
                    };
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id,
                        title,
                        buffer,
                        save_password,
                    };
                }
                KeyCode::Enter => {
                    self.overlay = OverlayMode::None;
                    self.authenticate_and_connect(&buffer, save_password);
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id,
                        title,
                        buffer,
                        save_password,
                    };
                }
                _ => {}
            },
            OverlayMode::HostKeyConfirm {
                endpoint,
                fingerprint,
            } => match ev.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.overlay = OverlayMode::None;
                    self.pending_handshake = None;
                    self.status_message = "Host key rejected. Connection cancelled.".to_string();
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.known_hosts.trust(&endpoint, &fingerprint);
                    self.save_known_hosts();
                    self.overlay = OverlayMode::None;
                    if let Some(pending) = self.pending_handshake.take() {
                        self.finish_connect(pending, None);
                    }
                }
                _ => {}
            },
            OverlayMode::ConnectionManager { mut cursor } => match ev.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.overlay = OverlayMode::None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if !self.remote_profiles.is_empty() {
                        cursor = (cursor + 1).min(self.remote_profiles.len() - 1);
                    }
                    self.overlay = OverlayMode::ConnectionManager { cursor };
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                    self.overlay = OverlayMode::ConnectionManager { cursor };
                }
                KeyCode::Char('a') | KeyCode::Char('n') => {
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new: true,
                        profile: ConnectionProfile::default(),
                        cursor: 0,
                        password_buffer: String::new(),
                    };
                }
                KeyCode::Char('e') => {
                    if let Some(p) = self.remote_profiles.get(cursor).cloned() {
                        let pw = credential::load_password(&p.credential_target()).unwrap_or_default();
                        self.overlay = OverlayMode::ConnectionEdit {
                            is_new: false,
                            profile: p,
                            cursor: 0,
                            password_buffer: pw,
                        };
                    }
                }
                KeyCode::Char('d') => {
                    if cursor < self.remote_profiles.len() {
                        let p = self.remote_profiles.remove(cursor);
                        credential::delete_password(&p.credential_target());
                        self.save_remote_profiles();
                        self.rebuild_tree();
                        if cursor >= self.remote_profiles.len() && cursor > 0 {
                            cursor -= 1;
                        }
                        self.overlay = OverlayMode::ConnectionManager { cursor };
                        self.status_message = format!("Removed profile: {}", p.display_label());
                    }
                }
                KeyCode::Enter => {
                    if let Some(p) = self.remote_profiles.get(cursor).cloned() {
                        self.overlay = OverlayMode::None;
                        self.connect_profile(&p.id, None);
                    }
                }
                _ => {}
            },
            OverlayMode::ConnectionEdit {
                is_new,
                mut profile,
                mut cursor,
                mut password_buffer,
            } => match ev.code {
                KeyCode::Esc => {
                    self.overlay = OverlayMode::ConnectionManager { cursor: 0 };
                }
                KeyCode::Up | KeyCode::BackTab => {
                    cursor = cursor.saturating_sub(1);
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Down | KeyCode::Tab => {
                    cursor = (cursor + 1).min(6);
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Char(' ') if cursor == 5 => {
                    profile.save_password = !profile.save_password;
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Backspace => {
                    match cursor {
                        0 => { profile.name.pop(); }
                        1 => { profile.host.pop(); }
                        2 => {
                            let mut s = profile.port.to_string();
                            s.pop();
                            profile.port = s.parse::<u16>().unwrap_or(0);
                        }
                        3 => { profile.user.pop(); }
                        4 => { password_buffer.pop(); }
                        6 => { profile.initial_dir.pop(); }
                        _ => {}
                    }
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Char(c) => {
                    match cursor {
                        0 => { profile.name.push(c); }
                        1 => { profile.host.push(c); }
                        2 => {
                            if c.is_ascii_digit() {
                                let mut s = profile.port.to_string();
                                if s == "0" || s == "22" && profile.port == 22 {
                                    s.clear();
                                }
                                s.push(c);
                                if let Ok(port) = s.parse::<u16>() {
                                    profile.port = port;
                                }
                            }
                        }
                        3 => { profile.user.push(c); }
                        4 => { password_buffer.push(c); }
                        6 => { profile.initial_dir.push(c); }
                        _ => {}
                    }
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Enter => {
                    if profile.id.is_empty() {
                        profile.id = format!("{}_{}", profile.user, profile.host);
                    }
                    if profile.port == 0 {
                        profile.port = 22;
                    }
                    if profile.save_password && !password_buffer.is_empty() {
                        let _ = credential::save_password(&profile.credential_target(), &profile.user, &password_buffer);
                    } else if !profile.save_password {
                        credential::delete_password(&profile.credential_target());
                    }
                    if is_new {
                        self.remote_profiles.push(profile);
                    } else if let Some(p) = self.remote_profiles.iter_mut().find(|p| p.id == profile.id) {
                        *p = profile;
                    }
                    self.save_remote_profiles();
                    self.rebuild_tree();
                    self.overlay = OverlayMode::ConnectionManager { cursor: 0 };
                    self.status_message = "Connection profile saved".to_string();
                }
                _ => {}
            },
        }
    }

    fn execute_menubar_item(&mut self, cat_idx: usize, item_idx: usize) {
        match cat_idx {
            0 => {
                // File: Open, Open with Default App, New File, New Folder, Rename, Delete, Create ZIP, Extract ZIP, [External CUI tools...], Exit, Exit & Cd
                let ext_count = self.external_apps.len();
                match item_idx {
                    0 => self.execute_action(ActionId::Open),
                    1 => self.execute_action(ActionId::OpenDefault),
                    2 => self.execute_action(ActionId::NewFile),
                    3 => self.execute_action(ActionId::NewFolder),
                    4 => self.execute_action(ActionId::Rename),
                    5 => self.execute_action(ActionId::Delete),
                    6 => self.execute_action(ActionId::ZipCreate),
                    7 => self.execute_action(ActionId::ZipExtract),
                    i if i >= 8 && i < 8 + ext_count => self.trigger_external_app(i - 8),
                    i if i == 8 + ext_count => self.execute_action(ActionId::Exit),
                    _ => self.execute_action(ActionId::ExitAndCd),
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
                4 => self.execute_action(ActionId::JumpAdd),
                5 => self.execute_action(ActionId::FuzzyFindFile),
                6 => self.execute_action(ActionId::FuzzyLiveGrep),
                7 => self.execute_action(ActionId::FuzzyJumpDir),
                _ => self.execute_action(ActionId::ConnectionManager),
            },
        }
    }

    fn commit_input_prompt(&mut self, kind: InputPromptKind, value: &str) {
        let trimmed = value.trim();
        match kind {
            InputPromptKind::PathBar => {
                if !trimmed.is_empty() {
                    if self.is_remote() && trimmed.starts_with('/') {
                        if let Some(session) = &self.remote_session {
                            let pid = session.profile.id.clone();
                            self.navigate_to_remote(&pid, trimmed);
                            return;
                        }
                    }
                    self.navigate_to(PathBuf::from(trimmed));
                }
            }
            InputPromptKind::Search => {
                self.search_query = value.to_string();
                self.apply_filter();
            }
            InputPromptKind::Rename { target } => {
                if !trimmed.is_empty() {
                    let was_current_dir = normalize_tree_key(&self.current_dir) == normalize_tree_key(&target);
                    match rename::rename_entry(&target, trimmed) {
                        Ok(new_path) => {
                            if was_current_dir {
                                self.current_dir = new_path;
                                self.expand_ancestors_of_current_dir();
                            }
                            self.reload_dir();
                            self.status_message = format!("Renamed to: {trimmed}");
                        }
                        Err(e) => {
                            self.status_message = format!("Rename failed: {e:?}");
                        }
                    }
                }
            }
            InputPromptKind::RenameRemote { target } => {
                if !trimmed.is_empty() {
                    if let Some(session) = &self.remote_session {
                        let parent = rpath::parent(&target).unwrap_or_else(|| "/".to_string());
                        let new_path = rpath::join(&parent, trimmed);
                        match session.rename(&target, &new_path) {
                            Ok(()) => {
                                if self.remote_dir.as_deref() == Some(&target) {
                                    self.remote_dir = Some(new_path);
                                }
                                self.reload_dir();
                                self.status_message = format!("Renamed remote to: {trimmed}");
                            }
                            Err(e) => {
                                self.status_message = format!("Rename failed: {e}");
                            }
                        }
                    }
                }
            }
            InputPromptKind::NewFile => {
                if !trimmed.is_empty() {
                    match self.target_destination() {
                        TargetDestination::Local(dest_dir) => {
                            match create::create_entry(&dest_dir, trimmed, "file") {
                                Ok(()) => {
                                    self.reload_dir();
                                    self.status_message = format!("Created file: {trimmed}");
                                }
                                Err(e) => {
                                    self.status_message = format!("Failed to create file: {e:?}");
                                }
                            }
                        }
                        TargetDestination::Remote { profile_id, path } => {
                            if let Some(session) = self
                                .remote_session
                                .as_ref()
                                .filter(|s| s.profile.id == profile_id)
                            {
                                let rtarget = rpath::join(&path, trimmed);
                                match session.create_file(&rtarget) {
                                    Ok(()) => {
                                        self.reload_dir();
                                        self.status_message =
                                            format!("Created remote file: {trimmed}");
                                    }
                                    Err(e) => {
                                        self.status_message =
                                            format!("Failed to create remote file: {e}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
            InputPromptKind::NewFolder => {
                if !trimmed.is_empty() {
                    match self.target_destination() {
                        TargetDestination::Local(dest_dir) => {
                            match create::create_entry(&dest_dir, trimmed, "dir") {
                                Ok(()) => {
                                    self.expanded_dirs.insert(normalize_tree_key(&dest_dir));
                                    self.reload_dir();
                                    self.status_message = format!("Created folder: {trimmed}");
                                }
                                Err(e) => {
                                    self.status_message = format!("Failed to create folder: {e:?}");
                                }
                            }
                        }
                        TargetDestination::Remote { profile_id, path } => {
                            if let Some(session) = self
                                .remote_session
                                .as_ref()
                                .filter(|s| s.profile.id == profile_id)
                            {
                                let rtarget = rpath::join(&path, trimmed);
                                match session.create_dir(&rtarget) {
                                    Ok(()) => {
                                        self.reload_dir();
                                        self.status_message =
                                            format!("Created remote folder: {trimmed}");
                                    }
                                    Err(e) => {
                                        self.status_message =
                                            format!("Failed to create remote folder: {e}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
            InputPromptKind::ZipCreate => {
                let target_paths = self.operation_targets();
                let base_dir_path = if self.focus_pane == FocusPane::Tree {
                    target_paths
                        .first()
                        .and_then(|p| p.parent().map(Path::to_path_buf))
                        .unwrap_or_else(|| self.current_dir.clone())
                } else {
                    self.current_dir.clone()
                };
                let targets: Vec<String> = target_paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                let base_dir = base_dir_path.to_string_lossy().into_owned();
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
            InputPromptKind::ConfirmDelete { .. } | InputPromptKind::ConfirmDeleteRemote { .. } => {}
        }
    }

    fn execute_action_menu_item(&mut self, cursor: usize) {
        match cursor {
            0 => self.execute_action(ActionId::Open),
            1 => self.execute_action(ActionId::OpenDefault),
            2 => self.execute_action(ActionId::Copy),
            3 => self.execute_action(ActionId::Cut),
            4 => self.execute_action(ActionId::Paste),
            5 => self.execute_action(ActionId::Rename),
            6 => self.execute_action(ActionId::Delete),
            7 => self.execute_action(ActionId::NewFile),
            8 => self.execute_action(ActionId::NewFolder),
            9 => self.execute_action(ActionId::ZipCreate),
            10 => self.execute_action(ActionId::ZipExtract),
            ext_idx => self.trigger_external_app(ext_idx - ACTION_MENU_FIXED_COUNT),
        }
    }

    fn check_and_record_click(&mut self, target: ClickTarget) -> bool {
        let now = Instant::now();
        let is_double = match self.last_click {
            Some((prev_time, prev_target)) => {
                prev_target == target && now.duration_since(prev_time) <= Duration::from_millis(500)
            }
            None => false,
        };
        if is_double {
            self.last_click = None;
        } else {
            self.last_click = Some((now, target));
        }
        is_double
    }

    pub fn handle_mouse_event(&mut self, ev: MouseEvent) {
        if self.active_transfer.is_some() {
            return;
        }

        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: self.term_width,
            height: self.term_height,
        };

        if self.overlay != OverlayMode::None {
            self.handle_overlay_mouse(ev, area);
            return;
        }

        // Top menu bar (row 0)
        if ev.row == 0 {
            if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(cat_idx) = hit_test_menubar_category(ev.column) {
                    self.overlay = OverlayMode::MenuBar {
                        cat_idx,
                        item_idx: 0,
                    };
                }
            }
            return;
        }

        let (path_bar_rect, tree_rect, file_list_rect) = crate::tui::main_panes_geometry(
            area,
            !self.search_query.is_empty(),
            self.ui_state.ui_show_tree,
            self.layout.tree_width_percent,
        );

        // Path / Search bar
        if ev.row >= path_bar_rect.y && ev.row < path_bar_rect.y + path_bar_rect.height {
            if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                if ev.row == path_bar_rect.y {
                    self.execute_action(ActionId::FocusPath);
                } else {
                    self.execute_action(ActionId::Search);
                }
            }
            return;
        }

        // Drives / Tree pane
        if let Some(t_rect) = tree_rect {
            if ev.column >= t_rect.x
                && ev.column < t_rect.x + t_rect.width
                && ev.row >= t_rect.y
                && ev.row < t_rect.y + t_rect.height
            {
                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        self.focus_pane = FocusPane::Tree;
                        self.tree_cursor = self.tree_cursor.saturating_sub(3);
                    }
                    MouseEventKind::ScrollDown => {
                        self.focus_pane = FocusPane::Tree;
                        if !self.tree_nodes.is_empty() {
                            self.tree_cursor =
                                (self.tree_cursor + 3).min(self.tree_nodes.len() - 1);
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.focus_pane = FocusPane::Tree;
                        if ev.row > t_rect.y
                            && ev.row < t_rect.y + t_rect.height.saturating_sub(1)
                            && ev.column > t_rect.x
                            && ev.column < t_rect.x + t_rect.width.saturating_sub(1)
                        {
                            let page_size = t_rect.height.saturating_sub(2).max(1) as usize;
                            let start_offset = (self.tree_cursor / page_size) * page_size;
                            let clicked_idx = start_offset + (ev.row - (t_rect.y + 1)) as usize;
                            if let Some(node) = self.tree_nodes.get(clicked_idx).cloned() {
                                self.tree_cursor = clicked_idx;
                                let rel_x = (ev.column - (t_rect.x + 1)) as usize;
                                let is_double =
                                    self.check_and_record_click(ClickTarget::TreeItem(clicked_idx));
                                let clicked_caret = rel_x < node.depth * 2 + 2;
                                if clicked_caret || is_double {
                                    let key = tree_node_key(&node);
                                    if node.expanded {
                                        self.expanded_dirs.remove(&key);
                                    } else {
                                        self.tree_children_cache.remove(&key);
                                        self.expanded_dirs.insert(key);
                                    }
                                    self.rebuild_tree();
                                }
                                if !clicked_caret {
                                    if let Some(remote) = &node.remote {
                                        match remote {
                                            RemoteNodeInfo::ProfileRoot(pid) => {
                                                self.connect_profile(pid, None);
                                            }
                                            RemoteNodeInfo::Directory { profile_id, path } => {
                                                self.focus_pane = FocusPane::FileList;
                                                self.navigate_to_remote(profile_id, path);
                                            }
                                        }
                                    } else {
                                        self.focus_pane = FocusPane::FileList;
                                        self.navigate_to(node.path);
                                    }
                                }
                            }
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.focus_pane = FocusPane::Tree;
                        if ev.row > t_rect.y
                            && ev.row < t_rect.y + t_rect.height.saturating_sub(1)
                            && ev.column > t_rect.x
                            && ev.column < t_rect.x + t_rect.width.saturating_sub(1)
                        {
                            let page_size = t_rect.height.saturating_sub(2).max(1) as usize;
                            let start_offset = (self.tree_cursor / page_size) * page_size;
                            let clicked_idx = start_offset + (ev.row - (t_rect.y + 1)) as usize;
                            if clicked_idx < self.tree_nodes.len() {
                                self.tree_cursor = clicked_idx;
                            }
                        }
                        self.overlay = OverlayMode::ActionMenu { cursor: 0 };
                    }
                    _ => {}
                }
                return;
            }
        }

        // File List pane (Windows Explorer-style interaction)
        if ev.column >= file_list_rect.x
            && ev.column < file_list_rect.x + file_list_rect.width
            && ev.row >= file_list_rect.y
            && ev.row < file_list_rect.y + file_list_rect.height
        {
            match ev.kind {
                MouseEventKind::ScrollUp => {
                    self.focus_pane = FocusPane::FileList;
                    self.focused_vis_idx = self.focused_vis_idx.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    self.focus_pane = FocusPane::FileList;
                    if !self.visible_indices.is_empty() {
                        self.focused_vis_idx =
                            (self.focused_vis_idx + 3).min(self.visible_indices.len() - 1);
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    self.focus_pane = FocusPane::FileList;
                    if let Some(vis_idx) = crate::tui::hit_test_file_list(
                        file_list_rect,
                        self.visible_indices.len(),
                        self.focused_vis_idx,
                        self.ui_state.ui_show_size,
                        self.ui_state.ui_show_time,
                        self.layout.min_col_width,
                        (ev.column, ev.row),
                    ) {
                        let prev_idx = self.focused_vis_idx;
                        self.focused_vis_idx = vis_idx;
                        if ev.modifiers.contains(KeyModifiers::CONTROL) {
                            self.last_click = None;
                            if let Some(entry) = self.focused_entry().cloned() {
                                if !self.selected_paths.remove(&entry.path) {
                                    self.selected_paths.insert(entry.path);
                                }
                            }
                        } else if ev.modifiers.contains(KeyModifiers::SHIFT) {
                            self.last_click = None;
                            let lo = prev_idx.min(vis_idx);
                            let hi = prev_idx.max(vis_idx);
                            for i in lo..=hi {
                                if let Some(&real_idx) = self.visible_indices.get(i) {
                                    if let Some(e) = self.entries.get(real_idx) {
                                        self.selected_paths.insert(e.path.clone());
                                    }
                                }
                            }
                        } else {
                            let is_double =
                                self.check_and_record_click(ClickTarget::FileItem(vis_idx));
                            if is_double {
                                self.execute_action(ActionId::Open);
                            } else {
                                self.selected_paths.clear();
                            }
                        }
                    } else {
                        self.last_click = None;
                        self.selected_paths.clear();
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => {
                    self.focus_pane = FocusPane::FileList;
                    self.last_click = None;
                    if let Some(vis_idx) = crate::tui::hit_test_file_list(
                        file_list_rect,
                        self.visible_indices.len(),
                        self.focused_vis_idx,
                        self.ui_state.ui_show_size,
                        self.ui_state.ui_show_time,
                        self.layout.min_col_width,
                        (ev.column, ev.row),
                    ) {
                        self.focused_vis_idx = vis_idx;
                        if let Some(entry) = self.focused_entry() {
                            if !self.selected_paths.is_empty()
                                && !self.selected_paths.contains(&entry.path)
                            {
                                self.selected_paths.clear();
                            }
                        }
                    }
                    self.overlay = OverlayMode::ActionMenu { cursor: 0 };
                }
                _ => {}
            }
        }
    }

    fn handle_overlay_mouse(&mut self, ev: MouseEvent, area: ratatui::layout::Rect) {
        match self.overlay.clone() {
            OverlayMode::None => {}
            OverlayMode::Input { .. } => {
                if ev.kind == MouseEventKind::Down(MouseButton::Left)
                    || ev.kind == MouseEventKind::Down(MouseButton::Right)
                {
                    let (popup, _, _) = crate::tui::popup_list_geometry(area, 1, 0);
                    if ev.column < popup.x
                        || ev.column >= popup.x + popup.width
                        || ev.row < popup.y
                        || ev.row >= popup.y + popup.height
                    {
                        self.overlay = OverlayMode::None;
                    }
                }
            }
            OverlayMode::ActionMenu { mut cursor } => {
                let total = ACTION_MENU_FIXED_COUNT + self.external_apps.len();
                let (popup, start_offset, page_size) =
                    crate::tui::popup_list_geometry(area, total, cursor);
                let in_inner = ev.column > popup.x
                    && ev.column < popup.x + popup.width.saturating_sub(1)
                    && ev.row > popup.y
                    && ev.row < popup.y + popup.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    MouseEventKind::ScrollDown => {
                        cursor = (cursor + 1).min(total.saturating_sub(1));
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    MouseEventKind::Moved if in_inner => {
                        let rel_y = (ev.row - (popup.y + 1)) as usize;
                        let idx = start_offset + rel_y;
                        if idx < total && rel_y < page_size {
                            self.overlay = OverlayMode::ActionMenu { cursor: idx };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let rel_y = (ev.row - (popup.y + 1)) as usize;
                            let idx = start_offset + rel_y;
                            if idx < total && rel_y < page_size {
                                self.overlay = OverlayMode::None;
                                self.execute_action_menu_item(idx);
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
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
                let total = SORT_OPTIONS.len();
                let (popup, start_offset, page_size) =
                    crate::tui::popup_list_geometry(area, total, cursor);
                let in_inner = ev.column > popup.x
                    && ev.column < popup.x + popup.width.saturating_sub(1)
                    && ev.row > popup.y
                    && ev.row < popup.y + popup.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    MouseEventKind::ScrollDown => {
                        cursor = (cursor + 1).min(total - 1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    MouseEventKind::Moved if in_inner => {
                        let rel_y = (ev.row - (popup.y + 1)) as usize;
                        let idx = start_offset + rel_y;
                        if idx < total && rel_y < page_size {
                            self.overlay = OverlayMode::SortMenu { cursor: idx };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let rel_y = (ev.row - (popup.y + 1)) as usize;
                            let idx = start_offset + rel_y;
                            if idx < total && rel_y < page_size {
                                let (k, o, label) = SORT_OPTIONS[idx];
                                self.ui_state.sort_key = k.to_string();
                                self.ui_state.sort_order = o.to_string();
                                self.save_ui_state();
                                self.overlay = OverlayMode::None;
                                self.reload_dir();
                                self.status_message = format!("Sorted by: {label}");
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::ListPopup {
                title,
                items,
                mut cursor,
                is_jump_list,
            } => {
                let total = items.len().max(1);
                let (popup, start_offset, page_size) =
                    crate::tui::popup_list_geometry(area, total, cursor);
                let in_inner = ev.column > popup.x
                    && ev.column < popup.x + popup.width.saturating_sub(1)
                    && ev.row > popup.y
                    && ev.row < popup.y + popup.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::ListPopup {
                            title,
                            items,
                            cursor,
                            is_jump_list,
                        };
                    }
                    MouseEventKind::ScrollDown => {
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
                    MouseEventKind::Moved if in_inner && !items.is_empty() => {
                        let rel_y = (ev.row - (popup.y + 1)) as usize;
                        let idx = start_offset + rel_y;
                        if idx < items.len() && rel_y < page_size {
                            self.overlay = OverlayMode::ListPopup {
                                title,
                                items,
                                cursor: idx,
                                is_jump_list,
                            };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let rel_y = (ev.row - (popup.y + 1)) as usize;
                            let idx = start_offset + rel_y;
                            self.overlay = OverlayMode::None;
                            if let Some(target_str) = items.get(idx) {
                                self.navigate_to(PathBuf::from(target_str));
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::MenuBar {
                cat_idx,
                mut item_idx,
            } => {
                if ev.row == 0 {
                    if let Some(new_cat) = hit_test_menubar_category(ev.column) {
                        if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                            if new_cat == cat_idx {
                                self.overlay = OverlayMode::None;
                            } else {
                                self.overlay = OverlayMode::MenuBar {
                                    cat_idx: new_cat,
                                    item_idx: 0,
                                };
                            }
                        } else if ev.kind == MouseEventKind::Moved && new_cat != cat_idx {
                            self.overlay = OverlayMode::MenuBar {
                                cat_idx: new_cat,
                                item_idx: 0,
                            };
                        }
                    } else if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                        self.overlay = OverlayMode::None;
                    }
                    return;
                }

                let cat_counts = [10usize + self.external_apps.len(), 6, 5, 9];
                let total = cat_counts[cat_idx];
                let rect = crate::tui::menubar_dropdown_rect(area, cat_idx, total);
                let in_inner = ev.column > rect.x
                    && ev.column < rect.x + rect.width.saturating_sub(1)
                    && ev.row > rect.y
                    && ev.row < rect.y + rect.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        item_idx = item_idx.saturating_sub(1);
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    MouseEventKind::ScrollDown => {
                        item_idx = (item_idx + 1).min(total.saturating_sub(1));
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    MouseEventKind::Moved if in_inner => {
                        let idx = (ev.row - (rect.y + 1)) as usize;
                        if idx < total {
                            self.overlay = OverlayMode::MenuBar {
                                cat_idx,
                                item_idx: idx,
                            };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let idx = (ev.row - (rect.y + 1)) as usize;
                            if idx < total {
                                self.overlay = OverlayMode::None;
                                self.execute_menubar_item(cat_idx, idx);
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::Viewer {
                path,
                title,
                is_markdown,
                mut preview_mode,
                raw_lines,
                preview_lines,
                mut scroll_y,
                mut scroll_x,
                search_query,
                search_input,
                mut matching_lines,
                mut match_cursor,
            } => {
                let active_len = if preview_mode {
                    preview_lines
                        .as_ref()
                        .map(|v| v.len())
                        .unwrap_or(raw_lines.len())
                } else {
                    raw_lines.len()
                };
                match ev.kind {
                    MouseEventKind::ScrollDown => {
                        scroll_y = (scroll_y + 3).min(active_len.saturating_sub(1));
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    MouseEventKind::ScrollUp => {
                        scroll_y = scroll_y.saturating_sub(3);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    MouseEventKind::Down(MouseButton::Right) if is_markdown => {
                        if let Some(ref prev) = preview_lines {
                            preview_mode = !preview_mode;
                            let new_len = if preview_mode { prev.len() } else { raw_lines.len() };
                            scroll_y = scroll_y.min(new_len.saturating_sub(1));
                            scroll_x = 0;
                            let new_active = if preview_mode { prev.as_slice() } else { raw_lines.as_slice() };
                            if let Some(ref q) = search_query {
                                matching_lines = crate::viewer::highlight::find_matching_lines(new_active, q);
                                match_cursor = if matching_lines.is_empty() {
                                    None
                                } else {
                                    Some(matching_lines.iter().position(|&l| l >= scroll_y).unwrap_or(0))
                                };
                            }
                        }
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    _ => {}
                }
            }
            OverlayMode::PasswordPrompt { .. }
            | OverlayMode::HostKeyConfirm { .. }
            | OverlayMode::ConnectionManager { .. }
            | OverlayMode::ConnectionEdit { .. } => {
                if ev.kind == MouseEventKind::Down(MouseButton::Right) {
                    self.overlay = OverlayMode::None;
                }
            }
        }
    }
}

fn hit_test_menubar_category(column: u16) -> Option<usize> {
    match column {
        0..=9 => Some(0),
        10..=19 => Some(1),
        20..=29 => Some(2),
        30..=37 => Some(3),
        _ => None,
    }
}
