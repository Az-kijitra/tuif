//! Copy, cut, paste and delete.

use super::*;

impl AppState {
    pub(super) fn execute_file_op_action(&mut self, action: ActionId) {
        match action {
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
                        let permanent =
                            action == ActionId::DeletePermanent || !self.general.delete_to_trash;
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
                    let permanent =
                        action == ActionId::DeletePermanent || !self.general.delete_to_trash;
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
            _ => unreachable!("routed by execute_action"),
        }
    }
}
