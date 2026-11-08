//! Action dispatch (ActionId) and launching viewers / external apps.

use super::*;

impl AppState {
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
                    self.focused_vis_idx =
                        (self.focused_vis_idx + 1).min(self.visible_indices.len() - 1);
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
                    self.focused_vis_idx =
                        (self.focused_vis_idx + rows).min(self.visible_indices.len() - 1);
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
                    if self.ui_state.ui_show_hidden {
                        "ON"
                    } else {
                        "OFF"
                    }
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
            ActionId::JumpAdd => {
                let p = self.active_target_dir().to_string_lossy().into_owned();
                self.nav_data
                    .jump_list
                    .retain(|x| !x.eq_ignore_ascii_case(&p));
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
            LaunchTarget::from_entry(
                &entry.path,
                entry.entry_type == EntryType::Dir,
                &self.current_dir,
            )
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

    pub(super) fn open_in_viewer(&mut self, path: &Path) {
        use std::io::Read;
        let ext = classify::extension_of(path);
        let mut sample = Vec::new();
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.take(65_536).read_to_end(&mut sample);
        }
        let kind = classify::decide_kind(ext.as_deref(), &sample);
        match kind {
            classify::ViewerKind::Text | classify::ViewerKind::Markdown => {
                match text::read_text(path) {
                    Ok(payload) => {
                        let is_markdown = kind == classify::ViewerKind::Markdown;
                        let raw_lines =
                            crate::viewer::highlight::highlight_with_bat(path, &payload.content);
                        let preview_lines = if is_markdown {
                            let width = self.term_width.saturating_sub(6).max(40) as usize;
                            crate::viewer::highlight::render_markdown_with_leaf(
                                &payload.content,
                                width,
                            )
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
                }
            }
            classify::ViewerKind::Image => {
                self.status_message = "Image preview is not supported in TUI viewer".to_string();
            }
            classify::ViewerKind::OsOpen => {
                self.status_message = "Binary files cannot be displayed in the built-in viewer (use external tools like bat)".to_string();
            }
        }
    }
}
