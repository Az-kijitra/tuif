//! Copy / move / paste between local and remote, conflict resolution, transfer progress.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConflictPolicy {
    Ask,
    OverwriteAll,
    SkipAll,
}

pub(super) fn resolve_conflict(
    policy: &std::sync::Mutex<ConflictPolicy>,
    file_name: &str,
    tx: &mpsc::Sender<TransferMessage>,
) -> ConflictChoice {
    let pol = policy.lock().unwrap();
    match *pol {
        ConflictPolicy::OverwriteAll => ConflictChoice::Overwrite,
        ConflictPolicy::SkipAll => ConflictChoice::Skip,
        ConflictPolicy::Ask => {
            let (reply_tx, reply_rx) = mpsc::channel();
            if tx
                .send(TransferMessage::ConflictPrompt {
                    file_name: file_name.to_string(),
                    reply_tx,
                })
                .is_err()
            {
                return ConflictChoice::Cancel;
            }
            drop(pol);
            match reply_rx.recv() {
                Ok(ConflictChoice::Overwrite) => ConflictChoice::Overwrite,
                Ok(ConflictChoice::Skip) => ConflictChoice::Skip,
                Ok(ConflictChoice::OverwriteAll) => {
                    *policy.lock().unwrap() = ConflictPolicy::OverwriteAll;
                    ConflictChoice::Overwrite
                }
                Ok(ConflictChoice::SkipAll) => {
                    *policy.lock().unwrap() = ConflictPolicy::SkipAll;
                    ConflictChoice::Skip
                }
                Ok(ConflictChoice::Cancel) | Err(_) => ConflictChoice::Cancel,
            }
        }
    }
}

impl AppState {
    pub fn poll_active_transfer(&mut self) {
        let Some(transfer) = self.active_transfer.as_mut() else {
            return;
        };

        let mut finished_msg = None;
        loop {
            match transfer.rx.try_recv() {
                Ok(TransferMessage::Progress(state)) => {
                    transfer.current_state = state;
                }
                Ok(TransferMessage::ConflictPrompt {
                    file_name,
                    reply_tx,
                }) => {
                    transfer.conflict_prompt = Some(ConflictPromptData {
                        file_name,
                        reply_tx,
                        cursor: 0,
                    });
                    break;
                }
                Ok(TransferMessage::Finished {
                    succeeded,
                    failed,
                    cancelled,
                    error_msg,
                    is_cut,
                }) => {
                    finished_msg = Some((succeeded, failed, cancelled, error_msg, is_cut));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.active_transfer = None;
                    self.status_message = "Transfer aborted (worker disconnected)".to_string();
                    self.reload_dir();
                    return;
                }
            }
        }

        if let Some((succeeded, failed, cancelled, error_msg, is_cut)) = finished_msg {
            self.active_transfer = None;
            if cancelled {
                self.status_message = format!("Transfer cancelled ({succeeded} completed)");
            } else if let Some(err) = error_msg {
                self.status_message = format!("Transfer error: {err}");
            } else {
                self.status_message =
                    format!("Transfer complete: {succeeded} succeeded / {failed} failed");
            }
            if is_cut && !cancelled && failed == 0 {
                self.clipboard = None;
                crate::fs_ops::os_clipboard::clear_os_clipboard();
            }
            self.reload_dir();
        }
    }

    pub(super) fn execute_paste(&mut self, clip: FileClipboard) {
        if self.active_transfer.is_some() {
            self.status_message = "Another transfer is currently in progress".to_string();
            return;
        }

        let dest = self.target_destination();
        match dest {
            TargetDestination::Local(dest_dir) => {
                let mut local_items = Vec::new();
                let mut remote_downloads = Vec::new();
                for item in &clip.items {
                    match item {
                        ClipboardItem::Local(p) => local_items.push(p.clone()),
                        ClipboardItem::Remote { profile_id, path } => {
                            remote_downloads.push((profile_id.clone(), path.clone()));
                        }
                    }
                }

                // If only local items exist, copy/move them in background with conflict resolution
                if remote_downloads.is_empty() {
                    if local_items.is_empty() {
                        return;
                    }
                    let cancel_flag = Arc::new(AtomicBool::new(false));
                    let (tx, rx) = mpsc::channel();
                    let is_cut = clip.mode == ClipMode::Cut;
                    let cancel_clone = Arc::clone(&cancel_flag);
                    let dest_clone = dest_dir.clone();
                    let items_clone = local_items.clone();

                    std::thread::spawn(move || {
                        let mut total_files = 0;
                        let mut total_bytes = 0;
                        for p in &items_clone {
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
                            let (f_count, b_count) = session::calc_local_size(p);
                            total_files += f_count;
                            total_bytes += b_count;
                        }

                        let _ = tx.send(TransferMessage::Progress(TransferProgressState {
                            title: if is_cut {
                                "Moving files...".to_string()
                            } else {
                                "Copying files...".to_string()
                            },
                            current_file: String::new(),
                            current_file_bytes: 0,
                            current_file_total: 0,
                            total_bytes_transferred: 0,
                            total_bytes,
                            files_transferred: 0,
                            total_files,
                            is_cancelling: false,
                        }));

                        let policy = std::sync::Mutex::new(ConflictPolicy::Ask);
                        let tx_clone = tx.clone();
                        let cancel_for_resolver = Arc::clone(&cancel_clone);
                        let dest_for_resolver = dest_clone.clone();

                        let resolver = move |item: &Path| {
                            let fname = item
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let target = dest_for_resolver.join(&fname);
                            if !target.exists() {
                                return transfer::ConflictResolution::Overwrite;
                            }
                            let choice = resolve_conflict(&policy, &fname, &tx_clone);
                            match choice {
                                ConflictChoice::Overwrite | ConflictChoice::OverwriteAll => {
                                    transfer::ConflictResolution::Overwrite
                                }
                                ConflictChoice::Skip | ConflictChoice::SkipAll => {
                                    transfer::ConflictResolution::Skip
                                }
                                ConflictChoice::Cancel => {
                                    cancel_for_resolver.store(true, Ordering::Relaxed);
                                    transfer::ConflictResolution::Reject
                                }
                            }
                        };

                        let cancel_check = || cancel_clone.load(Ordering::Relaxed);
                        let tx_for_event = tx.clone();
                        let title_str = if is_cut {
                            "Moving files...".to_string()
                        } else {
                            "Copying files...".to_string()
                        };
                        let on_event = move |ev: transfer::ProgressEvent| {
                            if ev.kind == transfer::ProgressEventKind::Start {
                                let _ = tx_for_event.send(TransferMessage::Progress(
                                    TransferProgressState {
                                        title: title_str.clone(),
                                        current_file: ev.name,
                                        current_file_bytes: 0,
                                        current_file_total: 0,
                                        total_bytes_transferred: 0,
                                        total_bytes,
                                        files_transferred: ev.index.saturating_sub(1) as usize,
                                        total_files,
                                        is_cancelling: cancel_check(),
                                    },
                                ));
                            }
                        };

                        let summary = match clip.mode {
                            ClipMode::Copy => transfer::copy_batch_with_resolver(
                                &items_clone,
                                &dest_clone,
                                resolver,
                                cancel_check,
                                on_event,
                            ),
                            ClipMode::Cut => transfer::move_batch_with_resolver(
                                &items_clone,
                                &dest_clone,
                                resolver,
                                cancel_check,
                                on_event,
                            ),
                        };

                        let cancelled = cancel_clone.load(Ordering::Relaxed);
                        let _ = tx.send(TransferMessage::Finished {
                            succeeded: summary.ok as usize,
                            failed: summary.failed as usize,
                            cancelled,
                            error_msg: None,
                            is_cut,
                        });
                    });

                    self.active_transfer = Some(ActiveTransfer {
                        cancel_flag,
                        rx,
                        current_state: TransferProgressState {
                            title: if clip.mode == ClipMode::Cut {
                                "Moving files...".to_string()
                            } else {
                                "Copying files...".to_string()
                            },
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
                    return;
                }

                // Remote downloads: spawn background worker with progress channel
                let Some(session) = self
                    .remote_session
                    .as_ref()
                    .filter(|s| remote_downloads.iter().all(|(pid, _)| &s.profile.id == pid))
                    .cloned()
                else {
                    self.status_message =
                        "Active remote session does not match download items".to_string();
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
                        if let Ok((f_count, b_count)) =
                            session.calc_remote_size(rpath, &cancel_clone)
                        {
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
                                let _ = tx_clone.send(TransferMessage::Progress(
                                    TransferProgressState {
                                        title: "Downloading from Remote...".to_string(),
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
            TargetDestination::Remote {
                profile_id,
                path: dest_rdir,
            } => {
                let Some(session) = self
                    .remote_session
                    .as_ref()
                    .filter(|s| s.profile.id == profile_id)
                    .cloned()
                else {
                    self.status_message =
                        "Remote session is not active for destination".to_string();
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
                                    let temp_path =
                                        temp_dir.join(session::sanitize_windows_name(&fname));
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
                                                    files_transferred: base_files
                                                        + (p.completed_files / 2),
                                                    total_files,
                                                    is_cancelling: cancel_ref
                                                        .load(Ordering::Relaxed),
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
                                                        current_file_bytes: p
                                                            .file_bytes_transferred,
                                                        current_file_total: p.file_total_bytes,
                                                        total_bytes_transferred: base_bytes
                                                            + (p.total_bytes_transferred / 2),
                                                        total_bytes,
                                                        files_transferred: base_files
                                                            + p.completed_files,
                                                        total_files,
                                                        is_cancelling: cancel_ref
                                                            .load(Ordering::Relaxed),
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
    }
}
