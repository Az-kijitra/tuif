//! Cursor movement and selection in the file list and the tree.

use super::*;

impl AppState {
    pub(super) fn execute_cursor_action(&mut self, action: ActionId) {
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
            _ => unreachable!("routed by execute_action"),
        }
    }
}
