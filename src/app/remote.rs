//! SFTP connection lifecycle and remote navigation.

use super::*;

impl AppState {
    pub fn navigate_to_remote(&mut self, profile_id: &str, dir: &str) {
        let rdir = rpath::normalize(dir);
        let is_connected = self
            .remote_session
            .as_ref()
            .map(|s| s.profile.id == profile_id)
            .unwrap_or(false);
        if !is_connected {
            self.connect_profile(profile_id, Some(rdir));
            return;
        }

        self.remote_dir = Some(rdir.clone());
        self.selected_paths.clear();
        self.focused_vis_idx = 0;
        self.search_query.clear();

        for anc in rpath::ancestors_root_first(&rdir) {
            self.expanded_dirs
                .insert(format!("remote:{profile_id}:{anc}"));
        }
        self.expanded_dirs
            .insert(format!("remote-root:{profile_id}"));

        self.reload_dir();
        self.sync_tree_cursor_to_current_dir();
    }

    pub fn connect_profile(&mut self, profile_id: &str, target_dir: Option<String>) {
        let Some(profile) = self
            .remote_profiles
            .iter()
            .find(|p| p.id == profile_id)
            .cloned()
        else {
            self.status_message = format!("Connection profile not found: {profile_id}");
            return;
        };
        self.status_message = format!("Connecting to {}...", profile.display_label());

        match handshake(&profile, &self.known_hosts) {
            Ok(pending) => match pending.status {
                HostKeyStatus::Unknown => {
                    self.overlay = OverlayMode::HostKeyConfirm {
                        endpoint: profile.endpoint_key(),
                        fingerprint: pending.fingerprint.clone(),
                    };
                    self.pending_handshake = Some(pending);
                }
                HostKeyStatus::Mismatch { expected } => {
                    self.status_message = format!(
                        "HOST KEY MISMATCH for {}! Expected {expected}, got {}",
                        profile.endpoint_key(),
                        pending.fingerprint
                    );
                }
                HostKeyStatus::Trusted => {
                    self.finish_connect(pending, target_dir);
                }
            },
            Err(e) => {
                self.status_message = format!("Connection failed: {e}");
            }
        }
    }

    pub fn finish_connect(&mut self, pending: PendingHandshake, target_dir: Option<String>) {
        let profile = pending.profile.clone();
        let saved_pw = if profile.save_password {
            credential::load_password(&profile.credential_target())
        } else {
            None
        };

        if let Some(pw) = saved_pw {
            match pending.authenticate(&pw) {
                Ok(session) => {
                    let initial = target_dir.unwrap_or_else(|| session.initial_dir());
                    self.status_message = format!("Connected to {}", profile.display_label());
                    self.expanded_dirs
                        .insert(format!("remote-root:{}", profile.id));
                    self.remote_session = Some(Arc::new(session));
                    self.navigate_to_remote(&profile.id, &initial);
                }
                Err(e) => {
                    self.status_message = format!("Authentication error: {e}");
                    self.pending_handshake = None;
                    self.overlay = OverlayMode::PasswordPrompt {
                        profile_id: profile.id.clone(),
                        title: format!("Password for {}", profile.display_label()),
                        buffer: String::new(),
                        save_password: profile.save_password,
                    };
                }
            }
        } else {
            self.pending_handshake = Some(pending);
            self.overlay = OverlayMode::PasswordPrompt {
                profile_id: profile.id.clone(),
                title: format!("Password for {}", profile.display_label()),
                buffer: String::new(),
                save_password: profile.save_password,
            };
        }
    }

    pub fn authenticate_and_connect(&mut self, password: &str, save_pw: bool) {
        let Some(pending) = self.pending_handshake.take() else {
            self.status_message = "No pending connection".to_string();
            return;
        };
        let profile = pending.profile.clone();
        match pending.authenticate(password) {
            Ok(session) => {
                if save_pw {
                    let _ = credential::save_password(
                        &profile.credential_target(),
                        &profile.user,
                        password,
                    );
                }
                if let Some(p) = self.remote_profiles.iter_mut().find(|p| p.id == profile.id) {
                    p.save_password = save_pw;
                }
                self.save_remote_profiles();
                let initial = session.initial_dir();
                self.status_message = format!("Connected to {}", profile.display_label());
                self.expanded_dirs
                    .insert(format!("remote-root:{}", profile.id));
                self.remote_session = Some(Arc::new(session));
                self.navigate_to_remote(&profile.id, &initial);
            }
            Err(e) => {
                self.status_message = format!("Authentication error: {e}");
            }
        }
    }

    pub fn open_remote_in_viewer(&mut self, remote_path: &str) {
        let Some(session) = &self.remote_session else {
            return;
        };
        let temp_dir = std::env::temp_dir()
            .join("tuif-remote")
            .join(&session.profile.id);
        let _ = std::fs::create_dir_all(&temp_dir);
        let file_name = rpath::file_name(remote_path);
        let local_temp = temp_dir.join(crate::remote::session::sanitize_windows_name(&file_name));
        self.status_message = format!("Downloading {file_name} for viewing...");
        match session.download(remote_path, &local_temp) {
            Ok(_) => {
                let url_title = format!(
                    "{}/{}",
                    session.profile.url_prefix(),
                    remote_path.trim_start_matches('/')
                );
                self.open_in_viewer(&local_temp);
                if let OverlayMode::Viewer { ref mut title, .. } = self.overlay {
                    *title = url_title;
                }
            }
            Err(e) => {
                self.status_message = format!("Download failed: {e}");
            }
        }
    }
}
