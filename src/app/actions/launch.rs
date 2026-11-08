//! Opening files in the built-in viewer, the default app, or a configured external app.

use super::*;

impl AppState {
    pub fn trigger_external_app(&mut self, index: usize) {
        let Some(app) = self.external_apps.get(index).cloned() else {
            return;
        };
        let target = if self.focus_pane == FocusPane::Tree {
            if let Some(node) = self.tree_nodes.get(self.tree_cursor) {
                LaunchTarget::from_entry(&node.path, true, &self.current_dir)
            } else {
                LaunchTarget::from_current_dir(&self.current_dir)
            }
        } else if let Some(entry) = self.focused_entry() {
            LaunchTarget::from_entry(
                &entry.path,
                entry.entry_type == EntryType::Dir,
                &self.current_dir,
            )
        } else {
            LaunchTarget::from_current_dir(&self.current_dir)
        };

        let Some(exe) = resolve_executable(&app.program) else {
            self.status_message = format!(
                "Cannot launch '{}': executable '{}' not found in PATH",
                app.name, app.program
            );
            return;
        };

        let args = match build_args(&app, &target) {
            Ok(a) => a,
            Err(e) => {
                self.status_message = format!("Argument error for '{}': {e}", app.name);
                return;
            }
        };

        if app.is_gui {
            match crate::external_apps::spawn_gui_nonblocking(&exe, &args, &target.dir) {
                Ok(()) => {
                    self.status_message = format!("Launched {}", app.name);
                }
                Err(e) => {
                    self.status_message = e;
                }
            }
            return;
        }

        self.pending_launch = Some(PendingExternalLaunch {
            app_name: app.name,
            exe,
            args,
            working_dir: target.dir,
        });
    }

    /// Opens the focused item (or Tree node) with the Windows default associated application.
    /// Remote files are downloaded to a temp folder first; remote folders are not supported.
    pub fn open_focused_with_default_app(&mut self) {
        let local_target: PathBuf = if self.focus_pane == FocusPane::Tree {
            let Some(node) = self.tree_nodes.get(self.tree_cursor) else {
                return;
            };
            if node.remote.is_some() {
                self.status_message =
                    "Open with default app is not available for remote folders".to_string();
                return;
            }
            node.path.clone()
        } else if self.is_remote() {
            let Some(entry) = self.focused_entry().cloned() else {
                return;
            };
            if entry.entry_type == EntryType::Dir {
                self.status_message =
                    "Open with default app is not available for remote folders".to_string();
                return;
            }
            let Some(session) = &self.remote_session else {
                return;
            };
            let remote_path = entry.path.to_string_lossy().into_owned();
            let temp_dir = std::env::temp_dir()
                .join("tuif-remote")
                .join(&session.profile.id);
            let _ = std::fs::create_dir_all(&temp_dir);
            let file_name = rpath::file_name(&remote_path);
            let local_temp =
                temp_dir.join(crate::remote::session::sanitize_windows_name(&file_name));
            if let Err(e) = session.download(&remote_path, &local_temp) {
                self.status_message = format!("Download failed: {e}");
                return;
            }
            local_temp
        } else if let Some(entry) = self.focused_entry() {
            entry.path.clone()
        } else {
            return;
        };

        let name = local_target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| local_target.to_string_lossy().into_owned());
        match crate::external_apps::open_with_default_app(&local_target) {
            Ok(()) => self.status_message = format!("Opened with default app: {name}"),
            Err(e) => self.status_message = e,
        }
    }

    pub(in crate::app) fn open_in_viewer(&mut self, path: &Path) {
        use std::io::Read;
        let ext = classify::extension_of(path);
        let mut sample = Vec::new();
        if let Ok(f) = std::fs::File::open(path) {
            let _ = f.take(65_536).read_to_end(&mut sample);
        }
        let kind = classify::decide_kind(ext.as_deref(), &sample);
        match kind {
            classify::ViewerKind::Text | classify::ViewerKind::Markdown => {
                match text::read_text(path) {
                    Ok(payload) => {
                        let is_markdown = kind == classify::ViewerKind::Markdown;
                        let raw_lines =
                            crate::viewer::highlight::highlight_with_bat(path, &payload.content);
                        let preview_lines = if is_markdown {
                            let width = self.term_width.saturating_sub(6).max(40) as usize;
                            crate::viewer::highlight::render_markdown_with_leaf(
                                &payload.content,
                                width,
                            )
                        } else {
                            None
                        };
                        let preview_mode = preview_lines.is_some();
                        let title = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Viewer")
                            .to_string();
                        self.overlay = OverlayMode::Viewer {
                            path: path.to_path_buf(),
                            title,
                            is_markdown,
                            preview_mode,
                            raw_lines,
                            preview_lines,
                            scroll_y: 0,
                            scroll_x: 0,
                            search_query: None,
                            search_input: None,
                            matching_lines: Vec::new(),
                            match_cursor: None,
                        };
                    }
                    Err(e) => {
                        self.status_message = format!("Cannot open in viewer: {e}");
                    }
                }
            }
            classify::ViewerKind::Image => {
                self.status_message = "Image preview is not supported in TUI viewer".to_string();
            }
            classify::ViewerKind::OsOpen => {
                self.status_message = "Binary files cannot be displayed in the built-in viewer (use external tools like bat)".to_string();
            }
        }
    }
}
