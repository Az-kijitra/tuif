//! Keyboard input: global key handling and overlay (dialog/menu) keys.

use super::*;

impl AppState {
    pub fn handle_key_event(&mut self, ev: KeyEvent) {
        if let Some(transfer) = self.active_transfer.as_mut() {
            if let Some(prompt) = transfer.conflict_prompt.as_mut() {
                let choice = match ev.code {
                    KeyCode::Left | KeyCode::BackTab => {
                        prompt.cursor = if prompt.cursor == 0 {
                            4
                        } else {
                            prompt.cursor - 1
                        };
                        None
                    }
                    KeyCode::Right | KeyCode::Tab => {
                        prompt.cursor = (prompt.cursor + 1) % 5;
                        None
                    }
                    KeyCode::Char('o') | KeyCode::Char('O') => Some(ConflictChoice::Overwrite),
                    KeyCode::Char('s') | KeyCode::Char('S') => Some(ConflictChoice::Skip),
                    KeyCode::Char('a') | KeyCode::Char('A') => Some(ConflictChoice::OverwriteAll),
                    KeyCode::Char('k') | KeyCode::Char('K') => Some(ConflictChoice::SkipAll),
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                        Some(ConflictChoice::Cancel)
                    }
                    KeyCode::Enter => match prompt.cursor {
                        0 => Some(ConflictChoice::Overwrite),
                        1 => Some(ConflictChoice::Skip),
                        2 => Some(ConflictChoice::OverwriteAll),
                        3 => Some(ConflictChoice::SkipAll),
                        _ => Some(ConflictChoice::Cancel),
                    },
                    _ => None,
                };
                if let Some(chosen) = choice {
                    if chosen == ConflictChoice::Cancel {
                        transfer.cancel_flag.store(true, Ordering::Relaxed);
                        transfer.current_state.is_cancelling = true;
                        self.status_message = "Cancelling transfer...".to_string();
                    }
                    let _ = prompt.reply_tx.send(chosen);
                    transfer.conflict_prompt = None;
                }
                return;
            }

            if ev.code == KeyCode::Esc {
                transfer.cancel_flag.store(true, Ordering::Relaxed);
                transfer.current_state.is_cancelling = true;
                self.status_message = "Cancelling transfer...".to_string();
            }
            return;
        }

        if !matches!(self.overlay, OverlayMode::None) {
            self.handle_overlay_key(ev);
            return;
        }

        // Tab switches focus between Tree and FileList when tree is visible
        if ev.code == KeyCode::Tab && ev.modifiers.is_empty() {
            if self.ui_state.ui_show_tree {
                self.focus_pane = match self.focus_pane {
                    FocusPane::FileList => FocusPane::Tree,
                    FocusPane::Tree => FocusPane::FileList,
                };
            }
            return;
        }

        // Menu bar activation: F10 or Alt+F/E/V/G (without Ctrl)
        if ev.code == KeyCode::F(10) {
            self.overlay = OverlayMode::MenuBar {
                cat_idx: 0,
                item_idx: 0,
            };
            return;
        }
        if ev.modifiers == KeyModifiers::ALT {
            if let KeyCode::Char(c) = ev.code {
                let cat = match c.to_ascii_uppercase() {
                    'F' => Some(0),
                    'E' => Some(1),
                    'V' => Some(2),
                    'G' => Some(3),
                    _ => None,
                };
                if let Some(cat_idx) = cat {
                    self.overlay = OverlayMode::MenuBar {
                        cat_idx,
                        item_idx: 0,
                    };
                    return;
                }
            }
        }

        let Some(chord) = KeyChord::from_crossterm(ev) else {
            return;
        };

        // Check registered external app hotkeys
        for idx in 0..self.external_apps.len() {
            let matched = self.external_apps[idx]
                .key
                .as_ref()
                .is_some_and(|chords| chords.contains(&chord));
            if matched {
                self.trigger_external_app(idx);
                return;
            }
        }

        if let Some(action) = self.keymap.resolve(&chord) {
            self.execute_action(action);
        }
    }

    pub(super) fn handle_overlay_key(&mut self, ev: KeyEvent) {
        match self.overlay.clone() {
            OverlayMode::None => {}
            OverlayMode::Input {
                kind,
                title,
                mut buffer,
            } => {
                if let InputPromptKind::ConfirmDelete { permanent, targets } = kind {
                    match ev.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let permanent = permanent || !self.general.delete_to_trash;
                            let summary = if permanent {
                                delete::delete_permanently(&targets)
                            } else {
                                delete::delete_to_trash(&targets)
                            };
                            self.overlay = OverlayMode::None;
                            self.selected_paths.clear();
                            self.reload_dir();
                            self.status_message = format!(
                                "Delete complete: {} succeeded / {} failed",
                                summary.ok, summary.failed
                            );
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            self.overlay = OverlayMode::None;
                        }
                        _ => {}
                    }
                    return;
                }
                if let InputPromptKind::ConfirmDeleteRemote { targets } = kind {
                    match ev.code {
                        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                            self.overlay = OverlayMode::None;
                            if let Some(session) = &self.remote_session {
                                let mut succeeded = 0;
                                let mut failed = 0;
                                for t in &targets {
                                    match session.remove_recursive(t) {
                                        Ok(()) => succeeded += 1,
                                        Err(_) => failed += 1,
                                    }
                                }
                                self.selected_paths.clear();
                                self.reload_dir();
                                self.status_message = format!(
                                    "Remote delete complete: {succeeded} succeeded / {failed} failed"
                                );
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                            self.overlay = OverlayMode::None;
                        }
                        _ => {}
                    }
                    return;
                }

                match ev.code {
                    KeyCode::Esc => {
                        if kind == InputPromptKind::Search {
                            self.search_query.clear();
                            self.apply_filter();
                        }
                        self.overlay = OverlayMode::None;
                    }
                    KeyCode::Enter => {
                        self.overlay = OverlayMode::None;
                        self.commit_input_prompt(kind, &buffer);
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                        if kind == InputPromptKind::Search {
                            self.search_query = buffer.clone();
                            self.apply_filter();
                        }
                        self.overlay = OverlayMode::Input {
                            kind,
                            title,
                            buffer,
                        };
                    }
                    KeyCode::Char('r')
                        if ev.modifiers.contains(KeyModifiers::CONTROL)
                            && kind == InputPromptKind::Search =>
                    {
                        self.search_regex = !self.search_regex;
                        self.apply_filter();
                        self.overlay = OverlayMode::Input {
                            kind,
                            title,
                            buffer,
                        };
                    }
                    KeyCode::Char(c)
                        if !ev.modifiers.contains(KeyModifiers::CONTROL)
                            && !ev.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        buffer.push(c);
                        if kind == InputPromptKind::Search {
                            self.search_query = buffer.clone();
                            self.apply_filter();
                        }
                        self.overlay = OverlayMode::Input {
                            kind,
                            title,
                            buffer,
                        };
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
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1).min(SORT_OPTIONS.len() - 1);
                        self.overlay = OverlayMode::SortMenu { cursor };
                    }
                    KeyCode::Enter => {
                        let (k, o, label) = SORT_OPTIONS[cursor];
                        self.ui_state.sort_key = k.to_string();
                        self.ui_state.sort_order = o.to_string();
                        self.save_ui_state();
                        self.overlay = OverlayMode::None;
                        self.reload_dir();
                        self.status_message = format!("Sorted by: {label}");
                    }
                    _ => {}
                }
            }
            OverlayMode::ListPopup {
                title,
                mut items,
                mut cursor,
                is_jump_list,
            } => match ev.code {
                KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    cursor = cursor.saturating_sub(1);
                    self.overlay = OverlayMode::ListPopup {
                        title,
                        items,
                        cursor,
                        is_jump_list,
                    };
                }
                KeyCode::Down | KeyCode::Char('j') => {
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
                KeyCode::Delete | KeyCode::Char('d') => {
                    if cursor < items.len() {
                        items.remove(cursor);
                        if is_jump_list {
                            self.nav_data.jump_list = items.clone();
                        } else {
                            self.nav_data.visit_history = items.clone();
                        }
                        self.save_navigation();
                        if cursor >= items.len() && !items.is_empty() {
                            cursor = items.len() - 1;
                        }
                    }
                    self.overlay = OverlayMode::ListPopup {
                        title,
                        items,
                        cursor,
                        is_jump_list,
                    };
                }
                KeyCode::Enter => {
                    self.overlay = OverlayMode::None;
                    if let Some(target_str) = items.get(cursor) {
                        self.navigate_to(PathBuf::from(target_str));
                    }
                }
                _ => {}
            },
            OverlayMode::ActionMenu { mut cursor } => {
                let total = ACTION_MENU_FIXED_COUNT + self.external_apps.len();
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = cursor.saturating_sub(1);
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1).min(total.saturating_sub(1));
                        self.overlay = OverlayMode::ActionMenu { cursor };
                    }
                    KeyCode::Enter => {
                        self.overlay = OverlayMode::None;
                        self.execute_action_menu_item(cursor);
                    }
                    _ => {}
                }
            }
            OverlayMode::MenuBar {
                mut cat_idx,
                mut item_idx,
            } => {
                let cat_counts = [10usize + self.external_apps.len(), 6, 5, 9];
                match ev.code {
                    KeyCode::Esc | KeyCode::Char('q') => self.overlay = OverlayMode::None,
                    KeyCode::Left | KeyCode::Char('h') => {
                        cat_idx = if cat_idx == 0 { 3 } else { cat_idx - 1 };
                        item_idx = 0;
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        cat_idx = (cat_idx + 1) % 4;
                        item_idx = 0;
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        item_idx = item_idx.saturating_sub(1);
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        item_idx = (item_idx + 1).min(cat_counts[cat_idx].saturating_sub(1));
                        self.overlay = OverlayMode::MenuBar { cat_idx, item_idx };
                    }
                    KeyCode::Enter => {
                        self.overlay = OverlayMode::None;
                        self.execute_menubar_item(cat_idx, item_idx);
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
                mut search_query,
                mut search_input,
                mut matching_lines,
                mut match_cursor,
            } => {
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
                                matching_lines = crate::viewer::highlight::find_matching_lines(
                                    active_lines,
                                    &query,
                                );
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
                            self.status_message =
                                "leaf is not available for Markdown preview".to_string();
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
                        if ev.code == KeyCode::PageDown
                            || ev.modifiers.contains(KeyModifiers::CONTROL) =>
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
                        if ev.code == KeyCode::PageUp
                            || ev.modifiers.contains(KeyModifiers::CONTROL) =>
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
            OverlayMode::PasswordPrompt {
                profile_id,
                title,
                mut buffer,
                mut save_password,
            } => match ev.code {
                KeyCode::Esc => {
                    self.overlay = OverlayMode::None;
                    self.pending_handshake = None;
                    self.status_message = "Connection cancelled".to_string();
                }
                KeyCode::Tab => {
                    save_password = !save_password;
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id,
                        title,
                        buffer,
                        save_password,
                    };
                }
                KeyCode::Backspace => {
                    buffer.pop();
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id,
                        title,
                        buffer,
                        save_password,
                    };
                }
                KeyCode::Enter => {
                    self.overlay = OverlayMode::None;
                    self.authenticate_and_connect(&buffer, save_password);
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id,
                        title,
                        buffer,
                        save_password,
                    };
                }
                _ => {}
            },
            OverlayMode::HostKeyConfirm {
                endpoint,
                fingerprint,
            } => match ev.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.overlay = OverlayMode::None;
                    self.pending_handshake = None;
                    self.status_message = "Host key rejected. Connection cancelled.".to_string();
                }
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.known_hosts.trust(&endpoint, &fingerprint);
                    self.save_known_hosts();
                    self.overlay = OverlayMode::None;
                    if let Some(pending) = self.pending_handshake.take() {
                        self.finish_connect(pending, None);
                    }
                }
                _ => {}
            },
            OverlayMode::ConnectionManager { mut cursor } => match ev.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.overlay = OverlayMode::None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if !self.remote_profiles.is_empty() {
                        cursor = (cursor + 1).min(self.remote_profiles.len() - 1);
                    }
                    self.overlay = OverlayMode::ConnectionManager { cursor };
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    cursor = cursor.saturating_sub(1);
                    self.overlay = OverlayMode::ConnectionManager { cursor };
                }
                KeyCode::Char('a') | KeyCode::Char('n') => {
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new: true,
                        // Port starts empty (0) so typing does not append to "22"; saved as 22 if left empty.
                        profile: ConnectionProfile {
                            port: 0,
                            ..ConnectionProfile::default()
                        },
                        cursor: 0,
                        password_buffer: String::new(),
                    };
                }
                KeyCode::Char('e') => {
                    if let Some(p) = self.remote_profiles.get(cursor).cloned() {
                        let pw =
                            credential::load_password(&p.credential_target()).unwrap_or_default();
                        self.overlay = OverlayMode::ConnectionEdit {
                            is_new: false,
                            profile: p,
                            cursor: 0,
                            password_buffer: pw,
                        };
                    }
                }
                KeyCode::Char('d') => {
                    if cursor < self.remote_profiles.len() {
                        let p = self.remote_profiles.remove(cursor);
                        credential::delete_password(&p.credential_target());
                        self.save_remote_profiles();
                        self.rebuild_tree();
                        if cursor >= self.remote_profiles.len() && cursor > 0 {
                            cursor -= 1;
                        }
                        self.overlay = OverlayMode::ConnectionManager { cursor };
                        self.status_message = format!("Removed profile: {}", p.display_label());
                    }
                }
                KeyCode::Enter => {
                    if let Some(p) = self.remote_profiles.get(cursor).cloned() {
                        self.overlay = OverlayMode::None;
                        self.connect_profile(&p.id, None);
                    }
                }
                _ => {}
            },
            OverlayMode::ConnectionEdit {
                is_new,
                mut profile,
                mut cursor,
                mut password_buffer,
            } => match ev.code {
                KeyCode::Esc => {
                    self.overlay = OverlayMode::ConnectionManager { cursor: 0 };
                }
                KeyCode::Up | KeyCode::BackTab => {
                    cursor = cursor.saturating_sub(1);
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Down | KeyCode::Tab => {
                    cursor = (cursor + 1).min(6);
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Char(' ') if cursor == 5 => {
                    profile.save_password = !profile.save_password;
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Backspace => {
                    match cursor {
                        0 => {
                            profile.name.pop();
                        }
                        1 => {
                            profile.host.pop();
                        }
                        2 => {
                            let mut s = profile.port.to_string();
                            s.pop();
                            profile.port = s.parse::<u16>().unwrap_or(0);
                        }
                        3 => {
                            profile.user.pop();
                        }
                        4 => {
                            password_buffer.pop();
                        }
                        6 => {
                            profile.initial_dir.pop();
                        }
                        _ => {}
                    }
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Char(c) => {
                    match cursor {
                        0 => {
                            profile.name.push(c);
                        }
                        1 => {
                            profile.host.push(c);
                        }
                        2 => {
                            if c.is_ascii_digit() {
                                let mut s = profile.port.to_string();
                                if s == "0" {
                                    s.clear();
                                }
                                s.push(c);
                                if let Ok(port) = s.parse::<u16>() {
                                    profile.port = port;
                                }
                            }
                        }
                        3 => {
                            profile.user.push(c);
                        }
                        4 => {
                            password_buffer.push(c);
                        }
                        6 => {
                            profile.initial_dir.push(c);
                        }
                        _ => {}
                    }
                    self.overlay = OverlayMode::ConnectionEdit {
                        is_new,
                        profile,
                        cursor,
                        password_buffer,
                    };
                }
                KeyCode::Enter => {
                    if profile.id.is_empty() {
                        profile.id = format!("{}_{}", profile.user, profile.host);
                    }
                    if profile.port == 0 {
                        profile.port = 22;
                    }
                    if profile.save_password && !password_buffer.is_empty() {
                        let _ = credential::save_password(
                            &profile.credential_target(),
                            &profile.user,
                            &password_buffer,
                        );
                    } else if !profile.save_password {
                        credential::delete_password(&profile.credential_target());
                    }
                    if is_new {
                        self.remote_profiles.push(profile);
                    } else if let Some(p) =
                        self.remote_profiles.iter_mut().find(|p| p.id == profile.id)
                    {
                        *p = profile;
                    }
                    self.save_remote_profiles();
                    self.rebuild_tree();
                    self.overlay = OverlayMode::ConnectionManager { cursor: 0 };
                    self.status_message = "Connection profile saved".to_string();
                }
                _ => {}
            },
        }
    }
}
