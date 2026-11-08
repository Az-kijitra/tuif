//! Paste into a remote (SFTP) folder: rename within the server, upload, or remote-to-remote copy.

use super::*;

impl AppState {
    pub(super) fn paste_to_remote(
        &mut self,
        clip: FileClipboard,
        profile_id: String,
        dest_rdir: String,
    ) {
        let Some(session) = self
            .remote_session
            .as_ref()
            .filter(|s| s.profile.id == profile_id)
            .cloned()
        else {
            self.status_message = "Remote session is not active for destination".to_string();
            return;
        };

        // Check if this is a simple intra-remote Cut (rename)
        let is_pure_remote_cut = clip.mode == ClipMode::Cut
            && clip.items.iter().all(|item| match item {
                ClipboardItem::Remote { profile_id: p, .. } => p == &profile_id,
                _ => false,
            });

        if is_pure_remote_cut {
            let mut succeeded = 0;
            let mut failed = 0;
            for item in &clip.items {
                if let ClipboardItem::Remote { path: src_path, .. } = item {
                    let fname = rpath::file_name(src_path);
                    let rtarget = rpath::join(&dest_rdir, &fname);
                    match session.rename(src_path, &rtarget) {
                        Ok(()) => succeeded += 1,
                        Err(_) => failed += 1,
                    }
                }
            }
            if failed == 0 {
                self.clipboard = None;
            }
            self.reload_dir();
            self.status_message =
                format!("Remote paste complete: {succeeded} succeeded / {failed} failed");
            return;
        }

        // Otherwise, perform background transfer (upload or remote-to-remote copy)
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let is_cut = clip.mode == ClipMode::Cut;
        let cancel_clone = Arc::clone(&cancel_flag);
        let items = clip.items.clone();
        let target_dir = dest_rdir.clone();

        std::thread::spawn(move || {
            // Pre-scan total files and bytes
            let mut total_files = 0;
            let mut total_bytes = 0;
            for item in &items {
                if cancel_clone.load(Ordering::Relaxed) {
                    let _ = tx.send(TransferMessage::Finished {
                        succeeded: 0,
                        failed: 0,
                        cancelled: true,
                        error_msg: None,
                        is_cut,
                    });
                    return;
                }
                match item {
                    ClipboardItem::Local(local_path) => {
                        let (f_count, b_count) = session::calc_local_size(local_path);
                        total_files += f_count;
                        total_bytes += b_count;
                    }
                    ClipboardItem::Remote { path: src_path, .. } => {
                        if let Ok((f_count, b_count)) =
                            session.calc_remote_size(src_path, &cancel_clone)
                        {
                            total_files += f_count;
                            total_bytes += b_count;
                        }
                    }
                }
            }

            let _ = tx.send(TransferMessage::Progress(TransferProgressState {
                title: "Uploading to Remote...".to_string(),
                current_file: String::new(),
                current_file_bytes: 0,
                current_file_total: 0,
                total_bytes_transferred: 0,
                total_bytes,
                files_transferred: 0,
                total_files,
                is_cancelling: false,
            }));

            let mut total_transferred = 0u64;
            let mut total_completed_files = 0usize;
            let mut succeeded = 0;
            let mut failed = 0;
            let mut cancelled = false;
            let policy = std::sync::Mutex::new(ConflictPolicy::Ask);

            for item in &items {
                if cancel_clone.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }

                match item {
                    ClipboardItem::Local(local_path) => {
                        let fname = local_path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "unnamed".to_string());
                        let rtarget = rpath::join(&target_dir, &fname);
                        let tx_clone = tx.clone();
                        let cancel_ref = &cancel_clone;
                        let base_bytes = total_transferred;
                        let base_files = total_completed_files;

                        let res = session.upload_with_progress(
                            local_path,
                            &rtarget,
                            cancel_ref,
                            |p| {
                                let _ = tx_clone.send(TransferMessage::Progress(
                                    TransferProgressState {
                                        title: "Uploading to Remote...".to_string(),
                                        current_file: p.file_name.clone(),
                                        current_file_bytes: p.file_bytes_transferred,
                                        current_file_total: p.file_total_bytes,
                                        total_bytes_transferred: base_bytes
                                            + p.total_bytes_transferred,
                                        total_bytes,
                                        files_transferred: base_files + p.completed_files,
                                        total_files,
                                        is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                    },
                                ));
                            },
                            |f| resolve_conflict(&policy, f, &tx_clone),
                        );

                        match res {
                            Ok(stats) => {
                                succeeded += 1;
                                total_completed_files += stats.files as usize;
                                total_transferred += stats.bytes;
                                if is_cut {
                                    let _ = if local_path.is_dir() {
                                        std::fs::remove_dir_all(local_path)
                                    } else {
                                        std::fs::remove_file(local_path)
                                    };
                                }
                            }
                            Err(_) => {
                                if cancel_ref.load(Ordering::Relaxed) {
                                    cancelled = true;
                                    break;
                                }
                                failed += 1;
                            }
                        }
                    }
                    ClipboardItem::Remote {
                        profile_id: src_pid,
                        path: src_path,
                    } => {
                        if src_pid == &session.profile.id {
                            let fname = rpath::file_name(src_path);
                            let rtarget = rpath::join(&target_dir, &fname);
                            let temp_dir = std::env::temp_dir().join("tuif-copy");
                            let _ = std::fs::create_dir_all(&temp_dir);
                            let temp_path = temp_dir.join(session::sanitize_windows_name(&fname));
                            let cancel_ref = &cancel_clone;
                            let tx_clone = tx.clone();
                            let base_bytes = total_transferred;
                            let base_files = total_completed_files;

                            let dl_res = session.download_with_progress(
                                src_path,
                                &temp_path,
                                cancel_ref,
                                |p| {
                                    let _ = tx_clone.send(TransferMessage::Progress(
                                        TransferProgressState {
                                            title: "Copying Remote File (Downloading)..."
                                                .to_string(),
                                            current_file: p.file_name.clone(),
                                            current_file_bytes: p.file_bytes_transferred,
                                            current_file_total: p.file_total_bytes,
                                            total_bytes_transferred: base_bytes
                                                + (p.total_bytes_transferred / 2),
                                            total_bytes,
                                            files_transferred: base_files + (p.completed_files / 2),
                                            total_files,
                                            is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                        },
                                    ));
                                },
                                |_| ConflictChoice::Overwrite,
                            );

                            if dl_res.is_ok() && !cancel_ref.load(Ordering::Relaxed) {
                                let ul_res = session.upload_with_progress(
                                    &temp_path,
                                    &rtarget,
                                    cancel_ref,
                                    |p| {
                                        let _ = tx_clone.send(TransferMessage::Progress(
                                            TransferProgressState {
                                                title: "Copying Remote File (Uploading)..."
                                                    .to_string(),
                                                current_file: p.file_name.clone(),
                                                current_file_bytes: p.file_bytes_transferred,
                                                current_file_total: p.file_total_bytes,
                                                total_bytes_transferred: base_bytes
                                                    + (p.total_bytes_transferred / 2),
                                                total_bytes,
                                                files_transferred: base_files + p.completed_files,
                                                total_files,
                                                is_cancelling: cancel_ref.load(Ordering::Relaxed),
                                            },
                                        ));
                                    },
                                    |f| resolve_conflict(&policy, f, &tx_clone),
                                );
                                let _ = std::fs::remove_file(&temp_path);
                                if let Ok(stats) = ul_res {
                                    succeeded += 1;
                                    total_completed_files += stats.files as usize;
                                    total_transferred += stats.bytes;
                                } else {
                                    failed += 1;
                                }
                            } else {
                                let _ = std::fs::remove_file(&temp_path);
                                if cancel_ref.load(Ordering::Relaxed) {
                                    cancelled = true;
                                    break;
                                }
                                failed += 1;
                            }
                        } else {
                            failed += 1;
                        }
                    }
                }
            }

            let _ = tx.send(TransferMessage::Finished {
                succeeded,
                failed,
                cancelled,
                error_msg: None,
                is_cut,
            });
        });

        self.active_transfer = Some(ActiveTransfer {
            cancel_flag,
            rx,
            current_state: TransferProgressState {
                title: "Starting Upload...".to_string(),
                current_file: String::new(),
                current_file_bytes: 0,
                current_file_total: 0,
                total_bytes_transferred: 0,
                total_bytes: 0,
                files_transferred: 0,
                total_files: 0,
                is_cancelling: false,
            },
            conflict_prompt: None,
        });
    }
}
