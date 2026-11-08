//! Drives / Tree pane: building the local and remote tree.

use super::*;

impl AppState {
    pub(super) fn load_child_dirs(&mut self, dir: &Path) -> Vec<(String, PathBuf)> {
        let key = normalize_tree_key(dir);
        if let Some(cached) = self.tree_children_cache.get(&key) {
            return cached.clone();
        }
        let children: Vec<(String, PathBuf)> =
            query::fs_list_dir(dir, self.ui_state.ui_show_hidden, "name", "asc")
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.entry_type == EntryType::Dir)
                .map(|e| (e.name, e.path))
                .collect();
        self.tree_children_cache.insert(key, children.clone());
        children
    }

    pub(super) fn load_remote_child_dirs(
        &mut self,
        profile_id: &str,
        dir: &str,
    ) -> Vec<(String, PathBuf)> {
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

    pub(super) fn append_tree_hierarchy(
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
                                children.sort_by_key(|a| a.0.to_lowercase());
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

    pub(super) fn append_remote_tree_hierarchy(
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
            if let Some(idx) = self
                .tree_nodes
                .iter()
                .position(|n| tree_node_key(n) == target_key)
            {
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
}
