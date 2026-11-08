//! Paste when both the clipboard items and the destination are local.

use super::*;

impl AppState {
    pub(super) fn paste_local_to_local(
        &mut self,
        clip: FileClipboard,
        dest_dir: PathBuf,
        local_items: Vec<PathBuf>,
    ) {
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
                    let _ = tx_for_event.send(TransferMessage::Progress(TransferProgressState {
                        title: title_str.clone(),
                        current_file: ev.name,
                        current_file_bytes: 0,
                        current_file_total: 0,
                        total_bytes_transferred: 0,
                        total_bytes,
                        files_transferred: ev.index.saturating_sub(1) as usize,
                        total_files,
                        is_cancelling: cancel_check(),
                    }));
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
    }
}
