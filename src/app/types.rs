//! Shared types for the application state: overlays, clipboard, tree nodes, transfers.

use super::*;

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
    Rename {
        target: PathBuf,
    },
    RenameRemote {
        target: String,
    },
    NewFile,
    NewFolder,
    ZipCreate,
    ZipExtract {
        zip_path: PathBuf,
    },
    ConfirmDelete {
        permanent: bool,
        targets: Vec<PathBuf>,
    },
    ConfirmDeleteRemote {
        targets: Vec<String>,
    },
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
