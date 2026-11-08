//! Action dispatch (ActionId). Groups of actions live in `actions/`; launching viewers and external apps in `actions/launch.rs`.

use super::*;

mod cursor;
mod dialogs;
mod file_ops;
mod launch;

impl AppState {
    pub fn execute_action(&mut self, action: ActionId) {
        match action {
            ActionId::MoveUp
            | ActionId::MoveDown
            | ActionId::MoveLeft
            | ActionId::MoveRight
            | ActionId::MoveTop
            | ActionId::MoveBottom
            | ActionId::PageUp
            | ActionId::PageDown
            | ActionId::SelectToggle
            | ActionId::SelectAll
            | ActionId::ClearSelection => self.execute_cursor_action(action),
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
            ActionId::Copy
            | ActionId::Cut
            | ActionId::Paste
            | ActionId::Delete
            | ActionId::DeletePermanent => self.execute_file_op_action(action),
            ActionId::Rename
            | ActionId::NewFile
            | ActionId::NewFolder
            | ActionId::Search
            | ActionId::SortMenu
            | ActionId::History
            | ActionId::HistoryJumpList
            | ActionId::ZipCreate
            | ActionId::ZipExtract
            | ActionId::ActionMenu
            | ActionId::ConnectionManager => self.execute_dialog_action(action),
            ActionId::JumpAdd => {
                let p = self.active_target_dir().to_string_lossy().into_owned();
                self.nav_data
                    .jump_list
                    .retain(|x| !x.eq_ignore_ascii_case(&p));
                self.nav_data.jump_list.insert(0, p.clone());
                self.save_navigation();
                self.status_message = format!("Added to Jump List: {p}");
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
}
