//! Directory listing, filtering, navigation history, and operation targets.

use super::*;

impl AppState {
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

    pub fn apply_filter(&mut self) {
        if self.search_query.is_empty() {
            self.visible_indices = (0..self.entries.len()).collect();
        } else if self.search_regex {
            match RegexBuilder::new(&self.search_query)
                .case_insensitive(true)
                .build()
            {
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

    pub(super) fn record_visit(&mut self) {
        let p = self.current_dir.to_string_lossy().into_owned();
        self.nav_data
            .visit_history
            .retain(|x| !x.eq_ignore_ascii_case(&p));
        self.nav_data.visit_history.insert(0, p);
        if self.nav_data.visit_history.len() > 50 {
            self.nav_data.visit_history.truncate(50);
        }
        self.save_navigation();
    }

    pub(super) fn save_navigation(&self) {
        if let Ok(path) = paths::navigation_path() {
            let _ = crate::config::navigation::save(&path, &self.nav_data);
        }
    }

    pub(super) fn save_ui_state(&self) {
        if let Ok(path) = paths::ui_state_path() {
            let _ = crate::config::ui_state::save(&path, &self.ui_state);
        }
    }
}
