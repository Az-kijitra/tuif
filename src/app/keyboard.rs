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
                buffer,
            } => self.overlay_key_input(ev, kind, title, buffer),
            OverlayMode::SortMenu { cursor } => self.overlay_key_sort_menu(ev, cursor),
            OverlayMode::ListPopup {
                title,
                items,
                cursor,
                is_jump_list,
            } => self.overlay_key_list_popup(ev, title, items, cursor, is_jump_list),
            OverlayMode::ActionMenu { cursor } => self.overlay_key_action_menu(ev, cursor),
            OverlayMode::MenuBar { cat_idx, item_idx } => {
                self.overlay_key_menubar(ev, cat_idx, item_idx)
            }
            overlay @ OverlayMode::Viewer { .. } => self.overlay_key_viewer(ev, overlay),
            OverlayMode::PasswordPrompt {
                profile_id,
                title,
                buffer,
                save_password,
            } => self.overlay_key_password(ev, profile_id, title, buffer, save_password),
            OverlayMode::HostKeyConfirm {
                endpoint,
                fingerprint,
            } => self.overlay_key_host_key(ev, endpoint, fingerprint),
            OverlayMode::ConnectionManager { cursor } => {
                self.overlay_key_connection_manager(ev, cursor)
            }
            OverlayMode::ConnectionEdit {
                is_new,
                profile,
                cursor,
                password_buffer,
            } => self.overlay_key_connection_edit(ev, is_new, profile, cursor, password_buffer),
        }
    }

    /// Text prompts (rename, new file/folder, path bar, search, ZIP) and delete confirmations.
    pub(super) fn overlay_key_input(
        &mut self,
        ev: KeyEvent,
        kind: InputPromptKind,
        title: String,
        mut buffer: String,
    ) {
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

    /// Sort menu: choose a sort key and order.
    pub(super) fn overlay_key_sort_menu(&mut self, ev: KeyEvent, mut cursor: usize) {
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

    /// Visit history / jump list popup.
    pub(super) fn overlay_key_list_popup(
        &mut self,
        ev: KeyEvent,
        title: String,
        mut items: Vec<String>,
        mut cursor: usize,
        is_jump_list: bool,
    ) {
        match ev.code {
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
        }
    }

    /// Right-click / `;` action menu.
    pub(super) fn overlay_key_action_menu(&mut self, ev: KeyEvent, mut cursor: usize) {
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

    /// Menu bar (F10).
    pub(super) fn overlay_key_menubar(
        &mut self,
        ev: KeyEvent,
        mut cat_idx: usize,
        mut item_idx: usize,
    ) {
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
}
