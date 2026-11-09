//! Mouse handling in the main panes: the Drives / Tree pane and the file list.

use super::*;

impl AppState {
    pub(super) fn mouse_tree_pane(&mut self, ev: MouseEvent, t_rect: ratatui::layout::Rect) {
        match ev.kind {
            MouseEventKind::ScrollUp => {
                self.focus_pane = FocusPane::Tree;
                self.tree_cursor = self.tree_cursor.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                self.focus_pane = FocusPane::Tree;
                if !self.tree_nodes.is_empty() {
                    self.tree_cursor = (self.tree_cursor + 3).min(self.tree_nodes.len() - 1);
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
    }

    pub(super) fn mouse_file_list(
        &mut self,
        ev: MouseEvent,
        file_list_rect: ratatui::layout::Rect,
    ) {
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
                        let is_double = self.check_and_record_click(ClickTarget::FileItem(vis_idx));
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
