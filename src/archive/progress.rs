//! `archive/progress.rs` (SPEC-0300-0014 R-381, R-386; SPEC-0200-0001 §3.4.1,
//! §3.4.5 throttling incl. SHOULD-88). Tests: `archive/progress_tests.rs`.

use serde::Serialize;
use std::time::Duration;

/// Tauri event name for ZIP progress (R-386; distinct from `fs_ops://progress`).
pub const PROGRESS_EVENT_NAME: &str = "archive://progress";

/// Minimum interval between emitted progress events (§3.4.5).
pub const EMIT_INTERVAL: Duration = Duration::from_millis(50);

/// One entry-boundary notification handed to the `on_progress` hooks of
/// `create` / `extract` (1-based `index`, `1 <= index <= total`).
#[derive(Debug, Clone, Copy)]
pub struct ProgressEvent<'a> {
    /// Archive-internal path of the entry being processed (`/` separated).
    pub name: &'a str,
    pub index: usize,
    pub total: usize,
}

/// IPC payload of `archive://progress` (`{ operationId, name, index, total }`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveProgressDto {
    pub operation_id: String,
    pub name: String,
    pub index: usize,
    pub total: usize,
}

/// §3.4.5: emit the first entry, the last entry and any entry at least
/// `EMIT_INTERVAL` after the previous emit. `since_last_emit` is `None` before
/// the first emit.
pub fn should_emit(index: usize, total: usize, since_last_emit: Option<Duration>) -> bool {
    if index <= 1 || index >= total {
        return true;
    }
    match since_last_emit {
        None => true,
        Some(elapsed) => elapsed >= EMIT_INTERVAL,
    }
}
