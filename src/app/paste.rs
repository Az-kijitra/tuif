//! Copy / move / paste between local and remote, conflict resolution, transfer progress.

use super::*;

mod download;
mod local;
mod upload;

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
                    self.paste_local_to_local(clip, dest_dir, local_items);
                } else {
                    self.paste_remote_to_local(clip, dest_dir, remote_downloads);
                }
            }
            TargetDestination::Remote {
                profile_id,
                path: dest_rdir,
            } => self.paste_to_remote(clip, profile_id, dest_rdir),
        }
    }
}
