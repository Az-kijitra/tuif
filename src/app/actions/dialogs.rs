//! Actions that open a dialog or menu (rename, new file/folder, search, history, ZIP, ...).

use super::*;

impl AppState {
    pub(super) fn execute_dialog_action(&mut self, action: ActionId) {
        match action {
            ActionId::Rename => {
                if self.focus_pane == FocusPane::Tree {
                    if let Some(node) = self.tree_nodes.get(self.tree_cursor).cloned() {
                        if let Some(RemoteNodeInfo::Directory { path, .. }) = node.remote {
                            self.overlay = OverlayMode::Input {
                                kind: InputPromptKind::RenameRemote { target: path },
                                title: "Rename Remote Folder (Enter:Apply / Esc:Cancel)"
                                    .to_string(),
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
                    title: "Search Filter (Ctrl+R:Toggle Regex / Enter:Keep / Esc:Clear)"
                        .to_string(),
                    buffer: self.search_query.clone(),
                };
            }
            ActionId::SortMenu => {
                self.overlay = OverlayMode::SortMenu { cursor: 0 };
            }
            ActionId::History => {
                self.overlay = OverlayMode::ListPopup {
                    title: "Visit History (j/k:Select / Enter:Go / d:Remove / Esc:Close)"
                        .to_string(),
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
                    kind: InputPromptKind::ZipExtract {
                        zip_path: entry.path,
                    },
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
            _ => unreachable!("routed by execute_action"),
        }
    }
}
