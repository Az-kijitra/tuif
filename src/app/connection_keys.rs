//! Keyboard handling for SFTP overlays: password and host-key prompts, connection manager and editor.

use super::*;

impl AppState {
    /// SFTP password prompt.
    pub(super) fn overlay_key_password(
        &mut self,
        ev: KeyEvent,
        profile_id: String,
        title: String,
        mut buffer: String,
        mut save_password: bool,
    ) {
        match ev.code {
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
        }
    }

    /// Unknown host key confirmation (TOFU).
    pub(super) fn overlay_key_host_key(
        &mut self,
        ev: KeyEvent,
        endpoint: String,
        fingerprint: String,
    ) {
        match ev.code {
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
        }
    }

    /// Connection profile list (Ctrl+S).
    pub(super) fn overlay_key_connection_manager(&mut self, ev: KeyEvent, mut cursor: usize) {
        match ev.code {
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
                    let pw = credential::load_password(&p.credential_target()).unwrap_or_default();
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
        }
    }

    /// Connection profile editor.
    pub(super) fn overlay_key_connection_edit(
        &mut self,
        ev: KeyEvent,
        is_new: bool,
        mut profile: ConnectionProfile,
        mut cursor: usize,
        mut password_buffer: String,
    ) {
        match ev.code {
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
                } else if let Some(p) = self.remote_profiles.iter_mut().find(|p| p.id == profile.id)
                {
                    *p = profile;
                }
                self.save_remote_profiles();
                self.rebuild_tree();
                self.overlay = OverlayMode::ConnectionManager { cursor: 0 };
                self.status_message = "Connection profile saved".to_string();
            }
            _ => {}
        }
    }
}
