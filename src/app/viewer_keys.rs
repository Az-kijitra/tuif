//! Keyboard handling for the built-in text / Markdown viewer overlay.

use super::*;

impl AppState {
    /// Built-in viewer: scrolling, preview toggle, and in-viewer search.
    pub(super) fn overlay_key_viewer(&mut self, ev: KeyEvent, overlay: OverlayMode) {
        let OverlayMode::Viewer {
            path,
            title,
            is_markdown,
            mut preview_mode,
            raw_lines,
            preview_lines,
            mut scroll_y,
            mut scroll_x,
            mut search_query,
            mut search_input,
            mut matching_lines,
            mut match_cursor,
        } = overlay
        else {
            return;
        };
        let active_lines = if preview_mode {
            preview_lines.as_ref().unwrap_or(&raw_lines)
        } else {
            &raw_lines
        };
        let active_len = active_lines.len();

        // If currently typing into the search prompt
        if let Some(mut input) = search_input {
            match ev.code {
                KeyCode::Esc => {
                    search_input = None;
                }
                KeyCode::Enter => {
                    let query = input.trim().to_string();
                    search_input = None;
                    if query.is_empty() {
                        search_query = None;
                        matching_lines.clear();
                        match_cursor = None;
                    } else {
                        matching_lines =
                            crate::viewer::highlight::find_matching_lines(active_lines, &query);
                        search_query = Some(query.clone());
                        if matching_lines.is_empty() {
                            match_cursor = None;
                            self.status_message = format!("Pattern not found: {query}");
                        } else {
                            let next_pos = matching_lines
                                .iter()
                                .position(|&l| l >= scroll_y)
                                .unwrap_or(0);
                            scroll_y = matching_lines[next_pos];
                            match_cursor = Some(next_pos);
                        }
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                    search_input = Some(input);
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    search_input = Some(input);
                }
                _ => {
                    search_input = Some(input);
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
            return;
        }

        match ev.code {
            KeyCode::Esc => {
                if search_query.is_some() {
                    search_query = None;
                    matching_lines.clear();
                    match_cursor = None;
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
                } else {
                    self.overlay = OverlayMode::None;
                }
            }
            KeyCode::Char('q') => self.overlay = OverlayMode::None,
            KeyCode::Char('/') | KeyCode::Char('f')
                if ev.code == KeyCode::Char('/')
                    || ev.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                search_input = Some(search_query.clone().unwrap_or_default());
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
            KeyCode::Char('n') => {
                if !matching_lines.is_empty() {
                    let cur = match_cursor.unwrap_or(0);
                    let next = (cur + 1) % matching_lines.len();
                    scroll_y = matching_lines[next];
                    match_cursor = Some(next);
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
            KeyCode::Char('N') => {
                if !matching_lines.is_empty() {
                    let cur = match_cursor.unwrap_or(0);
                    let prev = if cur == 0 {
                        matching_lines.len() - 1
                    } else {
                        cur - 1
                    };
                    scroll_y = matching_lines[prev];
                    match_cursor = Some(prev);
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
            KeyCode::Tab | KeyCode::Char('p') | KeyCode::Char('m') if is_markdown => {
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
                } else {
                    self.status_message = "leaf is not available for Markdown preview".to_string();
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
            KeyCode::Down | KeyCode::Char('j') => {
                scroll_y = (scroll_y + 1).min(active_len.saturating_sub(1));
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
            KeyCode::Up | KeyCode::Char('k') => {
                scroll_y = scroll_y.saturating_sub(1);
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
            KeyCode::PageDown | KeyCode::Char('d')
                if ev.code == KeyCode::PageDown || ev.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                scroll_y = (scroll_y + 20).min(active_len.saturating_sub(1));
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
            KeyCode::PageUp | KeyCode::Char('u')
                if ev.code == KeyCode::PageUp || ev.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                scroll_y = scroll_y.saturating_sub(20);
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
            KeyCode::Home | KeyCode::Char('g') => {
                scroll_y = 0;
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
            KeyCode::End | KeyCode::Char('G') => {
                scroll_y = active_len.saturating_sub(1);
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
            KeyCode::Right | KeyCode::Char('l') => {
                scroll_x = scroll_x.saturating_add(4);
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
            KeyCode::Left | KeyCode::Char('h') => {
                scroll_x = scroll_x.saturating_sub(4);
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
}
