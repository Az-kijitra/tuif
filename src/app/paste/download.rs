//! Paste remote (SFTP) items into a local folder.

use super::*;

impl AppState {
    pub(super) fn paste_remote_to_local(
        &mut self,
        clip: FileClipboard,
        dest_dir: PathBuf,
        remote_downloads: Vec<(String, String)>,
    ) {
        // Remote downloads: spawn background worker with progress channel
        let Some(session) = self
            .remote_session
            .as_ref()
            .filter(|s| remote_downloads.iter().all(|(pid, _)| &s.profile.id == pid))
            .cloned()
        else {
            self.status_message = "Active remote session does not match download items".to_string();
            return;
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let is_cut = clip.mode == ClipMode::Cut;
        let cancel_clone = Arc::clone(&cancel_flag);
        let dest_clone = dest_dir.clone();

        std::thread::spawn(move || {
            // Pre-scan total files and bytes
            let mut total_files = 0;
            let mut total_bytes = 0;
            for (_, rpath) in &remote_downloads {
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
                if let Ok((f_count, b_count)) = session.calc_remote_size(rpath, &cancel_clone) {
                    total_files += f_count;
                    total_bytes += b_count;
                }
            }

            let _ = tx.send(TransferMessage::Progress(TransferProgressState {
                title: "Downloading from Remote...".to_string(),
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

            for (_, rpath) in &remote_downloads {
                if cancel_clone.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                let fname = rpath::file_name(rpath);
                let local_target = dest_clone.join(session::sanitize_windows_name(&fname));
                let tx_clone = tx.clone();
                let cancel_ref = &cancel_clone;
                let base_bytes = total_transferred;
                let base_files = total_completed_files;

                let res = session.download_with_progress(
                    rpath,
                    &local_target,
                    cancel_ref,
                    |p| {
                        let _ = tx_clone.send(TransferMessage::Progress(TransferProgressState {
                            title: "Downloading from Remote...".to_string(),
                            current_file: p.file_name.clone(),
                            current_file_bytes: p.file_bytes_transferred,
                            current_file_total: p.file_total_bytes,
                            total_bytes_transferred: base_bytes + p.total_bytes_transferred,
                            total_bytes,
                            files_transferred: base_files + p.completed_files,
                            total_files,
                            is_cancelling: cancel_ref.load(Ordering::Relaxed),
                        }));
                    },
                    |f| resolve_conflict(&policy, f, &tx_clone),
                );

                match res {
                    Ok(stats) => {
                        succeeded += 1;
                        total_completed_files += stats.files as usize;
                        total_transferred += stats.bytes;
                        if is_cut {
                            let _ = session.remove_recursive(rpath);
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
                title: "Starting Download...".to_string(),
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
