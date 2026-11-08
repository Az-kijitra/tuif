//! Mouse input: clicks, double-clicks, wheel, and overlay hit-testing.

use super::*;

impl AppState {
    pub(super) fn check_and_record_click(&mut self, target: ClickTarget) -> bool {
        let now = Instant::now();
        let is_double = match self.last_click {
            Some((prev_time, prev_target)) => {
                prev_target == target && now.duration_since(prev_time) <= Duration::from_millis(500)
            }
            None => false,
        };
        if is_double {
            self.last_click = None;
        } else {
            self.last_click = Some((now, target));
        }
        is_double
    }

    pub fn handle_mouse_event(&mut self, ev: MouseEvent) {
        if self.active_transfer.is_some() {
            return;
        }

        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: self.term_width,
            height: self.term_height,
        };

        if self.overlay != OverlayMode::None {
            self.handle_overlay_mouse(ev, area);
            return;
        }

        // Top menu bar (row 0)
        if ev.row == 0 {
            if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(cat_idx) = hit_test_menubar_category(ev.column) {
                    self.overlay = OverlayMode::MenuBar {
                        cat_idx,
                        item_idx: 0,
                    };
                }
            }
            return;
        }

        let (path_bar_rect, tree_rect, file_list_rect) = crate::tui::main_panes_geometry(
            area,
            !self.search_query.is_empty(),
            self.ui_state.ui_show_tree,
            self.layout.tree_width_percent,
        );

        // Path / Search bar
        if ev.row >= path_bar_rect.y && ev.row < path_bar_rect.y + path_bar_rect.height {
            if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                if ev.row == path_bar_rect.y {
                    self.execute_action(ActionId::FocusPath);
                } else {
                    self.execute_action(ActionId::Search);
                }
            }
            return;
        }

        // Drives / Tree pane
        if let Some(t_rect) = tree_rect {
            if ev.column >= t_rect.x
                && ev.column < t_rect.x + t_rect.width
                && ev.row >= t_rect.y
                && ev.row < t_rect.y + t_rect.height
            {
                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        self.focus_pane = FocusPane::Tree;
                        self.tree_cursor = self.tree_cursor.saturating_sub(3);
                    }
                    MouseEventKind::ScrollDown => {
                        self.focus_pane = FocusPane::Tree;
                        if !self.tree_nodes.is_empty() {
                            self.tree_cursor =
                                (self.tree_cursor + 3).min(self.tree_nodes.len() - 1);
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
                return;
            }
        }

        // File List pane (Windows Explorer-style interaction)
        if ev.column >= file_list_rect.x
            && ev.column < file_list_rect.x + file_list_rect.width
            && ev.row >= file_list_rect.y
            && ev.row < file_list_rect.y + file_list_rect.height
        {
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
                            let is_double =
                                self.check_and_record_click(ClickTarget::FileItem(vis_idx));
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

    pub(super) fn handle_overlay_mouse(&mut self, ev: MouseEvent, area: ratatui::layout::Rect) {
        match self.overlay.clone() {
            OverlayMode::None => {}
            OverlayMode::Input { .. } => {
                if ev.kind == MouseEventKind::Down(MouseButton::Left)
                    || ev.kind == MouseEventKind::Down(MouseButton::Right)
                {
                    let (popup, _, _) = crate::tui::popup_list_geometry(area, 1, 0);
                    if ev.column < popup.x
                        || ev.column >= popup.x + popup.width
                        || ev.row < popup.y
                        || ev.row >= popup.y + popup.height
                    {
                        self.overlay = OverlayMode::None;
                    }
                }
            }
            OverlayMode::ActionMenu { mut cursor } => {
                let total = ACTION_MENU_FIXED_COUNT + self.external_apps.len();
                let (popup, start_offset, page_size) =
                    crate::tui::popup_list_geometry(area, total, cursor);
                let in_inner = ev.column > popup.x
                    && ev.column < popup.x + popup.width.saturating_sub(1)
                    && ev.row > popup.y
                    && ev.row < popup.y + popup.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    MouseEventKind::ScrollDown => {
                        cursor = (cursor + 1).min(total.saturating_sub(1));
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    MouseEventKind::Moved if in_inner => {
                        let rel_y = (ev.row - (popup.y + 1)) as usize;
                        let idx = start_offset + rel_y;
                        if idx < total && rel_y < page_size {
                            self.overlay = OverlayMode::ActionMenu { cursor: idx };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let rel_y = (ev.row - (popup.y + 1)) as usize;
                            let idx = start_offset + rel_y;
                            if idx < total && rel_y < page_size {
                                self.overlay = OverlayMode::None;
                                self.execute_action_menu_item(idx);
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::SortMenu { mut cursor } => {
                const SORT_OPTIONS: &[(&str, &str, &str)] = &[
                    ("name", "asc", "Name (Asc)"),
                    ("name", "desc", "Name (Desc)"),
                    ("size", "asc", "Size (Asc)"),
                    ("size", "desc", "Size (Desc)"),
                    ("type", "asc", "Type (Asc)"),
                    ("type", "desc", "Type (Desc)"),
                    ("modified", "asc", "Modified (Asc)"),
                    ("modified", "desc", "Modified (Desc)"),
                ];
                let total = SORT_OPTIONS.len();
                let (popup, start_offset, page_size) =
                    crate::tui::popup_list_geometry(area, total, cursor);
                let in_inner = ev.column > popup.x
                    && ev.column < popup.x + popup.width.saturating_sub(1)
                    && ev.row > popup.y
                    && ev.row < popup.y + popup.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    MouseEventKind::ScrollDown => {
                        cursor = (cursor + 1).min(total - 1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    MouseEventKind::Moved if in_inner => {
                        let rel_y = (ev.row - (popup.y + 1)) as usize;
                        let idx = start_offset + rel_y;
                        if idx < total && rel_y < page_size {
                            self.overlay = OverlayMode::SortMenu { cursor: idx };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let rel_y = (ev.row - (popup.y + 1)) as usize;
                            let idx = start_offset + rel_y;
                            if idx < total && rel_y < page_size {
                                let (k, o, label) = SORT_OPTIONS[idx];
                                self.ui_state.sort_key = k.to_string();
                                self.ui_state.sort_order = o.to_string();
                                self.save_ui_state();
                                self.overlay = OverlayMode::None;
                                self.reload_dir();
                                self.status_message = format!("Sorted by: {label}");
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::ListPopup {
                title,
                items,
                mut cursor,
                is_jump_list,
            } => {
                let total = items.len().max(1);
                let (popup, start_offset, page_size) =
                    crate::tui::popup_list_geometry(area, total, cursor);
                let in_inner = ev.column > popup.x
                    && ev.column < popup.x + popup.width.saturating_sub(1)
                    && ev.row > popup.y
                    && ev.row < popup.y + popup.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::ListPopup {
                            title,
                            items,
                            cursor,
                            is_jump_list,
                        };
                    }
                    MouseEventKind::ScrollDown => {
                        if !items.is_empty() {
                            cursor = (cursor + 1).min(items.len() - 1);
                        }
                        self.overlay = OverlayMode::ListPopup {
                            title,
                            items,
                            cursor,
                            is_jump_list,
                        };
                    }
                    MouseEventKind::Moved if in_inner && !items.is_empty() => {
                        let rel_y = (ev.row - (popup.y + 1)) as usize;
                        let idx = start_offset + rel_y;
                        if idx < items.len() && rel_y < page_size {
                            self.overlay = OverlayMode::ListPopup {
                                title,
                                items,
                                cursor: idx,
                                is_jump_list,
                            };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let rel_y = (ev.row - (popup.y + 1)) as usize;
                            let idx = start_offset + rel_y;
                            self.overlay = OverlayMode::None;
                            if let Some(target_str) = items.get(idx) {
                                self.navigate_to(PathBuf::from(target_str));
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::MenuBar {
                cat_idx,
                mut item_idx,
            } => {
                if ev.row == 0 {
                    if let Some(new_cat) = hit_test_menubar_category(ev.column) {
                        if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                            if new_cat == cat_idx {
                                self.overlay = OverlayMode::None;
                            } else {
                                self.overlay = OverlayMode::MenuBar {
                                    cat_idx: new_cat,
                                    item_idx: 0,
                                };
                            }
                        } else if ev.kind == MouseEventKind::Moved && new_cat != cat_idx {
                            self.overlay = OverlayMode::MenuBar {
                                cat_idx: new_cat,
                                item_idx: 0,
                            };
                        }
                    } else if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                        self.overlay = OverlayMode::None;
                    }
                    return;
                }

                let cat_counts = [10usize + self.external_apps.len(), 6, 5, 9];
                let total = cat_counts[cat_idx];
                let rect = crate::tui::menubar_dropdown_rect(area, cat_idx, total);
                let in_inner = ev.column > rect.x
                    && ev.column < rect.x + rect.width.saturating_sub(1)
                    && ev.row > rect.y
                    && ev.row < rect.y + rect.height.saturating_sub(1);

                match ev.kind {
                    MouseEventKind::ScrollUp => {
                        item_idx = item_idx.saturating_sub(1);
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    MouseEventKind::ScrollDown => {
                        item_idx = (item_idx + 1).min(total.saturating_sub(1));
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    MouseEventKind::Moved if in_inner => {
                        let idx = (ev.row - (rect.y + 1)) as usize;
                        if idx < total {
                            self.overlay = OverlayMode::MenuBar {
                                cat_idx,
                                item_idx: idx,
                            };
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if in_inner {
                            let idx = (ev.row - (rect.y + 1)) as usize;
                            if idx < total {
                                self.overlay = OverlayMode::None;
                                self.execute_menubar_item(cat_idx, idx);
                            }
                        } else {
                            self.overlay = OverlayMode::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.overlay = OverlayMode::None;
                    }
                    _ => {}
                }
            }
            OverlayMode::Viewer {
                path,
                title,
                is_markdown,
                mut preview_mode,
                raw_lines,
                preview_lines,
                mut scroll_y,
                mut scroll_x,
                search_query,
                search_input,
                mut matching_lines,
                mut match_cursor,
            } => {
                let active_len = if preview_mode {
                    preview_lines
                        .as_ref()
                        .map(|v| v.len())
                        .unwrap_or(raw_lines.len())
                } else {
                    raw_lines.len()
                };
                match ev.kind {
                    MouseEventKind::ScrollDown => {
                        scroll_y = (scroll_y + 3).min(active_len.saturating_sub(1));
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    MouseEventKind::ScrollUp => {
                        scroll_y = scroll_y.saturating_sub(3);
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    MouseEventKind::Down(MouseButton::Right) if is_markdown => {
                        if let Some(ref prev) = preview_lines {
                            preview_mode = !preview_mode;
                            let new_len = if preview_mode {
                                prev.len()
                            } else {
                                raw_lines.len()
                            };
                            scroll_y = scroll_y.min(new_len.saturating_sub(1));
                            scroll_x = 0;
                            let new_active = if preview_mode {
                                prev.as_slice()
                            } else {
                                raw_lines.as_slice()
                            };
                            if let Some(ref q) = search_query {
                                matching_lines =
                                    crate::viewer::highlight::find_matching_lines(new_active, q);
                                match_cursor = if matching_lines.is_empty() {
                                    None
                                } else {
                                    Some(
                                        matching_lines
                                            .iter()
                                            .position(|&l| l >= scroll_y)
                                            .unwrap_or(0),
                                    )
                                };
                            }
                        }
                        self.overlay = OverlayMode::Viewer {
                            path,
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y,
                            scroll_x,
                            search_query,
                            search_input,
                            matching_lines,
                            match_cursor,
                        };
                    }
                    _ => {}
                }
            }
            OverlayMode::PasswordPrompt { .. }
            | OverlayMode::HostKeyConfirm { .. }
            | OverlayMode::ConnectionManager { .. }
            | OverlayMode::ConnectionEdit { .. } => {
                if ev.kind == MouseEventKind::Down(MouseButton::Right) {
                    self.overlay = OverlayMode::None;
                }
            }
        }
    }
}

pub(super) fn hit_test_menubar_category(column: u16) -> Option<usize> {
    match column {
        0..=9 => Some(0),
        10..=19 => Some(1),
        20..=29 => Some(2),
        30..=37 => Some(3),
        _ => None,
    }
}
