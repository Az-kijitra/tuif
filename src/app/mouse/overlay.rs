//! Mouse handling while an overlay (dialog, menu, viewer) is open.

use super::*;

impl AppState {
    pub(super) fn handle_overlay_mouse(&mut self, ev: MouseEvent, area: ratatui::layout::Rect) {
        match self.overlay.clone() {
            OverlayMode::None => {}
            OverlayMode::Input { .. } => self.overlay_mouse_input(ev, area),
            OverlayMode::ActionMenu { cursor } => self.overlay_mouse_action_menu(ev, area, cursor),
            OverlayMode::SortMenu { cursor } => self.overlay_mouse_sort_menu(ev, area, cursor),
            overlay @ OverlayMode::ListPopup { .. } => {
                self.overlay_mouse_list_popup(ev, area, overlay)
            }
            OverlayMode::MenuBar { cat_idx, item_idx } => {
                self.overlay_mouse_menubar(ev, area, cat_idx, item_idx)
            }
            overlay @ OverlayMode::Viewer { .. } => self.overlay_mouse_viewer(ev, overlay),
            OverlayMode::PasswordPrompt { .. }
            | OverlayMode::HostKeyConfirm { .. }
            | OverlayMode::ConnectionManager { .. }
            | OverlayMode::ConnectionEdit { .. } => self.overlay_mouse_connection(ev),
        }
    }

    fn overlay_mouse_input(&mut self, ev: MouseEvent, area: ratatui::layout::Rect) {
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

    fn overlay_mouse_action_menu(
        &mut self,
        ev: MouseEvent,
        area: ratatui::layout::Rect,
        mut cursor: usize,
    ) {
        let total = ACTION_MENU_FIXED_COUNT + self.external_apps.len();
        let (popup, start_offset, page_size) = crate::tui::popup_list_geometry(area, total, cursor);
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

    fn overlay_mouse_sort_menu(
        &mut self,
        ev: MouseEvent,
        area: ratatui::layout::Rect,
        mut cursor: usize,
    ) {
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
        let (popup, start_offset, page_size) = crate::tui::popup_list_geometry(area, total, cursor);
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

    fn overlay_mouse_list_popup(
        &mut self,
        ev: MouseEvent,
        area: ratatui::layout::Rect,
        overlay: OverlayMode,
    ) {
        let OverlayMode::ListPopup {
            title,
            items,
            mut cursor,
            is_jump_list,
        } = overlay
        else {
            return;
        };
        let total = items.len().max(1);
        let (popup, start_offset, page_size) = crate::tui::popup_list_geometry(area, total, cursor);
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

    fn overlay_mouse_menubar(
        &mut self,
        ev: MouseEvent,
        area: ratatui::layout::Rect,
        cat_idx: usize,
        mut item_idx: usize,
    ) {
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

    fn overlay_mouse_viewer(&mut self, ev: MouseEvent, overlay: OverlayMode) {
        let OverlayMode::Viewer {
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
        } = overlay
        else {
            return;
        };
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

    fn overlay_mouse_connection(&mut self, ev: MouseEvent) {
        if ev.kind == MouseEventKind::Down(MouseButton::Right) {
            self.overlay = OverlayMode::None;
        }
    }
}
