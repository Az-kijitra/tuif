//! Mouse input: clicks, double-clicks, wheel, and overlay hit-testing.

use super::*;

mod overlay;
mod panes;

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
                self.mouse_tree_pane(ev, t_rect);
                return;
            }
        }

        // File List pane (Windows Explorer-style interaction)
        if ev.column >= file_list_rect.x
            && ev.column < file_list_rect.x + file_list_rect.width
            && ev.row >= file_list_rect.y
            && ev.row < file_list_rect.y + file_list_rect.height
        {
            self.mouse_file_list(ev, file_list_rect);
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
