//! Menu bar, action menu, and input prompt commits.

use super::*;

impl AppState {
    pub(super) fn execute_menubar_item(&mut self, cat_idx: usize, item_idx: usize) {
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

    pub(super) fn commit_input_prompt(&mut self, kind: InputPromptKind, value: &str) {
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
                    let was_current_dir =
                        normalize_tree_key(&self.current_dir) == normalize_tree_key(&target);
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
                            match create::create_entry(&dest_dir, trimmed, "folder") {
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
                match crate::archive::guarded::create_guarded(
                    &targets, trimmed, &base_dir, true, &hooks,
                ) {
                    Ok(res) => {
                        self.reload_dir();
                        self.status_message = format!(
                            "Created ZIP ({} entries): {}",
                            res.entries,
                            res.output_path.display()
                        );
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
                match crate::archive::guarded::extract_guarded(
                    &zip_str,
                    trimmed,
                    &base_dir,
                    &env,
                    &resolutions,
                    &hooks,
                ) {
                    Ok(res) => {
                        self.reload_dir();
                        self.status_message = format!(
                            "Extracted ZIP ({} files): {}",
                            res.extracted,
                            res.destination.display()
                        );
                    }
                    Err(e) => {
                        self.status_message = format!("ZIP extraction error: {}", e.message);
                    }
                }
            }
            InputPromptKind::ConfirmDelete { .. } | InputPromptKind::ConfirmDeleteRemote { .. } => {
            }
        }
    }

    pub(super) fn execute_action_menu_item(&mut self, cursor: usize) {
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
}
