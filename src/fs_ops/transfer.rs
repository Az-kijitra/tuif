//! SPEC-0300-0011 §4/§5/§10 (R-20~R-36, R-80, R-86): copy/move batch
//! execution, conflict handling, and the R-23 fallback-bug fix. Planned API
//! (not yet implemented — see `super::mod` doc comment):
//!
//! ```ignore
//! pub struct Failure { pub path: PathBuf, pub code: String, pub message: String }
//! pub struct OpSummary {
//!     pub ok: u32, pub failed: u32, pub total: u32, pub skipped: u32,
//!     pub failures: Vec<Failure>,
//! }
//! pub enum ConflictResolution { Reject, Overwrite, Skip }
//! pub fn copy_batch(items: &[PathBuf], destination: &Path, resolution: ConflictResolution) -> OpSummary;
//! pub fn move_batch(items: &[PathBuf], destination: &Path, resolution: ConflictResolution) -> OpSummary;
//!
//! // R-23: a small *pure* decision function isolating the fallback-bug fix
//! // from the actual OS rename call, so the fix itself is unit-testable
//! // without needing a real cross-filesystem rename failure. Both
//! // arguments are ordinary `Result<(), String>` values (the outcome of
//! // attempting `fs::rename` and, if that failed, the outcome of the
//! // recursive-copy-then-remove fallback) — this is plain decision logic,
//! // not a Storage Provider abstraction (SPEC-0400-0001 §4.1): no trait,
//! // no generics, no dyn dispatch over the filesystem.
//! pub fn resolve_move_item_outcome(
//!     rename_result: Result<(), String>,
//!     fallback_result: Option<Result<(), String>>,
//! ) -> Result<(), String>;
//! ```
//!
//! `ConflictResolution::Reject` is the non-UI-path default (R-30/R-30-1/
//! R-30-2/AC-29): a caller that supplies no explicit resolution must have
//! conflicting items fail with code `"conflict"`, never silently overwritten.
//!
//! **R-40~R-45 (progress/cancel, ISS-0052):** `copy_batch`/`move_batch`
//! above are unchanged, no-op-callback wrappers around
//! `copy_batch_with_progress`/`move_batch_with_progress`, which additionally
//! take a `should_cancel` poll closure (R-43) and an `on_event` callback
//! that receives one `ProgressEvent` per item boundary (R-40/R-41/R-44).
//! Both are plain `Fn`-trait parameters — no trait objects, no generic
//! backend abstraction (SPEC-0400-0001 §4.1) — so this stays unit-testable
//! without a Tauri runtime. `fs_ops::commands` wires these into actual
//! Tauri event emission and a `State`-backed per-operation cancel registry.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::shared::identity::{is_strictly_within, same_entity};
use super::preflight::{self, TransferPreflightError};

/// R-22: a single failed item within a batch result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub path: PathBuf,
    pub code: String,
    pub message: String,
}

/// R-22/R-51/R-56-3: the batch-result shape shared by copy/move/delete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpSummary {
    pub ok: u32,
    pub failed: u32,
    pub total: u32,
    /// R-25: items excluded from processing (e.g. same-directory paste, or
    /// an explicit "skip" conflict resolution) — never counted as failures.
    pub skipped: u32,
    pub failures: Vec<Failure>,
}

/// R-30/R-31: how a conflicting destination item is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// R-30/R-30-1/R-30-2: the non-UI-path default — reject with `conflict`.
    Reject,
    Overwrite,
    Skip,
}

/// R-23: pure decision logic isolating the fallback-bug fix (`main`'s bug:
/// returning the original rename error even when the copy+remove fallback
/// succeeded) from the actual OS rename call, so the fix is unit-testable
/// without needing a real cross-filesystem rename failure.
pub fn resolve_move_item_outcome(
    rename_result: Result<(), String>,
    fallback_result: Option<Result<(), String>>,
) -> Result<(), String> {
    match (rename_result, fallback_result) {
        (Ok(()), _) => Ok(()),
        (Err(_), Some(Ok(()))) => Ok(()),
        (Err(_), Some(Err(fallback_err))) => Err(fallback_err),
        (Err(rename_err), None) => Err(rename_err),
    }
}

fn copy_recursively(source: &Path, destination: &Path) -> Result<(), String> {
    if source.is_dir() {
        fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let file_type = entry.file_type().map_err(|e| e.to_string())?;
            let child_destination = destination.join(entry.file_name());
            if file_type.is_dir() {
                copy_recursively(&entry.path(), &child_destination)?;
            } else {
                fs::copy(entry.path(), &child_destination).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    } else {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(source, destination).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn remove_recursively(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(path).map_err(|e| e.to_string())
    }
}

/// R-20: which batch operation `run_transfer_batch` performs for each item.
/// A plain enum, not a generic backend parameter — this only selects
/// between the two concrete operations below (SPEC-0400-0001 §4.1: no
/// trait, no generics/dyn dispatch over the filesystem).
#[derive(Debug, Clone, Copy)]
enum TransferKind {
    Copy,
    Move,
}

fn run_transfer_item(kind: TransferKind, source: &Path, target: &Path) -> Result<(), ItemError> {
    match kind {
        TransferKind::Copy => copy_recursively(source, target).map_err(io_item_error),
        TransferKind::Move => {
            if fs::rename(source, target).is_ok() {
                return Ok(());
            }
            move_fallback(source, target)
        }
    }
}

/// R-23 fallback for a failed rename: copy then remove. The destination is
/// re-checked first: `fs::copy` overwrites, so a destination that appeared
/// after the conflict decision must fail with `conflict`, not be clobbered.
pub(crate) fn move_fallback(source: &Path, target: &Path) -> Result<(), ItemError> {
    if entry_exists(target) {
        return Err(ItemError {
            code: "conflict",
            message: "destination already exists".to_string(),
        });
    }
    resolve_move_item_outcome(
        Err("rename failed".to_string()),
        Some(copy_recursively(source, target).and_then(|_| remove_recursively(source))),
    )
    .map_err(io_item_error)
}

// ---------------------------------------------------------------------------
// Overwrite = file replace + folder merge (R-31, R-31-2~R-31-7, R-80;
// SPEC-0200-0001 §3.3.14-2). The destination is never deleted up front.
// ---------------------------------------------------------------------------

/// One failure found while overwriting/merging a single top-level item.
pub(crate) struct ItemError {
    pub(crate) code: &'static str,
    pub(crate) message: String,
}

fn io_item_error(err: impl ToString) -> ItemError {
    ItemError {
        code: "io_error",
        message: err.to_string(),
    }
}

/// Existence test that also sees dangling symlinks/junctions. The name
/// comparison itself follows the OS (case-insensitive on NTFS, R-32-2).
fn entry_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A not-yet-existing sibling name of `target` used as the staging file for
/// an atomic replace (R-31-5).
fn temp_sibling(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    loop {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".{name}.{}-{n}.rftmp", std::process::id()));
        if !entry_exists(&candidate) {
            return candidate;
        }
    }
}

/// Removes a staging file, clearing a read-only bit copied from the source
/// if that is what blocks the removal.
fn remove_temp(path: &Path) {
    if fs::remove_file(path).is_err() {
        if let Ok(meta) = fs::metadata(path) {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
            let _ = fs::remove_file(path);
        }
    }
}

/// R-31-5: replaces the file `target` with the content of `source` by
/// copying to a temporary sibling first and renaming it over `target`, so a
/// failure at any point leaves the original `target` intact (and no
/// temporary file behind).
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    let temp = temp_sibling(target);
    if let Err(err) = fs::copy(source, &temp) {
        remove_temp(&temp);
        return Err(err.to_string());
    }
    if let Err(err) = fs::rename(&temp, target) {
        remove_temp(&temp);
        return Err(err.to_string());
    }
    Ok(())
}

/// Move of a file onto an existing file: an atomic rename over the target,
/// falling back to staged replace + source removal when rename is not
/// possible (e.g. cross-volume). On any failure the source is kept.
fn move_replace_file(source: &Path, target: &Path) -> Result<(), String> {
    if fs::rename(source, target).is_ok() {
        return Ok(());
    }
    replace_file(source, target)?;
    fs::remove_file(source).map_err(|e| e.to_string())
}

/// Overwrites the existing `target` with `source` (both exist). Files are
/// replaced, folders are merged recursively, a kind mismatch fails with
/// `conflict` without touching either side (R-31-4). Failures are collected
/// into `errors` and processing of the remaining entries continues (R-80).
fn overwrite_entry(
    kind: TransferKind,
    source: &Path,
    source_is_dir: bool,
    target: &Path,
    errors: &mut Vec<ItemError>,
) {
    if source_is_dir != target.is_dir() {
        errors.push(ItemError {
            code: "conflict",
            message: format!(
                "cannot overwrite: `{}` and `{}` are not both files or both folders",
                source.display(),
                target.display()
            ),
        });
        return;
    }

    // MUST-59: never write over (or into) the source itself.
    match same_entity(source, target) {
        // The same entity: there is nothing to overwrite; leave it alone.
        Some(true) => return,
        Some(false) => {}
        None => {
            errors.push(ItemError {
                code: "io_error",
                message: format!("cannot verify that `{}` is a different item; not overwritten", target.display()),
            });
            return;
        }
    }
    if source_is_dir {
        let target_inside_source = is_strictly_within(source, target);
        let source_inside_target = is_strictly_within(target, source);
        if target_inside_source != Some(false) || source_inside_target != Some(false) {
            errors.push(ItemError {
                code: "overwrite_ancestor",
                message: format!("`{}` and `{}` are nested; not merged", source.display(), target.display()),
            });
            return;
        }
        merge_dir(kind, source, target, errors);
    } else {
        let result = match kind {
            TransferKind::Copy => replace_file(source, target),
            TransferKind::Move => move_replace_file(source, target),
        };
        if let Err(message) = result {
            errors.push(io_item_error(message));
        }
    }
}

/// Merges the folder `source` into the existing folder `target`: entries
/// only in `target` stay, missing entries are transferred, same-named
/// entries are overwritten recursively (R-31-2, R-31-3). For a move, the
/// source folder is removed afterwards only when every entry was transferred
/// and it is empty (R-31-7).
fn merge_dir(kind: TransferKind, source: &Path, target: &Path, errors: &mut Vec<ItemError>) {
    let errors_before = errors.len();
    match fs::read_dir(source) {
        Err(err) => errors.push(io_item_error(err)),
        Ok(entries) => {
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        errors.push(io_item_error(err));
                        continue;
                    }
                };
                let is_dir = match entry.file_type() {
                    Ok(file_type) => file_type.is_dir(),
                    Err(err) => {
                        errors.push(io_item_error(err));
                        continue;
                    }
                };
                let child_source = entry.path();
                let child_target = target.join(entry.file_name());
                if entry_exists(&child_target) {
                    overwrite_entry(kind, &child_source, is_dir, &child_target, errors);
                } else if let Err(item_error) = run_transfer_item(kind, &child_source, &child_target) {
                    errors.push(item_error);
                }
            }
        }
    }
    if matches!(kind, TransferKind::Move) && errors.len() == errors_before {
        // `remove_dir` fails on a non-empty folder, which is exactly the
        // "keep it if anything is left" rule; the failure is not an error.
        let _ = fs::remove_dir(source);
    }
}

/// R-24: error code reported for the whole batch when `preflight::preflight_transfer`
/// rejects it upfront (AC-72: zero side effects — no item is touched).
pub(crate) fn preflight_error_code(err: TransferPreflightError) -> &'static str {
    match err {
        TransferPreflightError::NoItems => "no_items",
        TransferPreflightError::SamePath => "same_path",
        TransferPreflightError::DestinationInsideSource => "destination_inside_source",
        TransferPreflightError::DestinationNotFound => "destination_not_found",
        TransferPreflightError::DestinationNotDir => "destination_not_dir",
        TransferPreflightError::InvalidPath => "invalid_path",
        TransferPreflightError::NotFound => "not_found",
    }
}

/// R-40: which lifecycle stage a `ProgressEvent` reports for a single item.
/// Mirrors the frozen frontend `ProgressEventType` union
/// (`app/src/lib/features/file-ops/progress-modal.ts`) — do not rename
/// these variants without updating `as_str` and that frontend contract
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEventKind {
    Start,
    Done,
    Fail,
}

impl ProgressEventKind {
    /// The exact wire string the frozen frontend `ProgressEvent.type` union
    /// expects (`"start" | "done" | "fail"`).
    pub fn as_str(self) -> &'static str {
        match self {
            ProgressEventKind::Start => "start",
            ProgressEventKind::Done => "done",
            ProgressEventKind::Fail => "fail",
        }
    }
}

/// R-40/R-41/R-44: one per-item progress notification, emitted immediately
/// before an item is attempted (`Start`) and immediately after it finishes
/// (`Done`/`Fail`). Items excluded from processing *without ever being
/// attempted* receive no `Start` event: R-25 same-directory pastes, an
/// explicit `Skip` conflict resolution, and any item reached after a
/// cancellation request (R-43) — the last of these still emits a lone
/// `Fail{code: Some("canceled")}` (R-44) so the UI can report exactly how
/// many items were left unprocessed, but the two "excluded" cases stay
/// completely silent, matching their "not attempted, not counted as
/// failure" semantics (R-25) and the fact the frozen frontend
/// `ProgressEvent` union has no "skip" variant.
///
/// Plain data, no Tauri dependency — this keeps the emission-timing and
/// cancellation logic below unit-testable without a Tauri runtime.
/// `fs_ops::commands` is the only place that turns this into a
/// `Serialize`-able DTO and emits it as a Tauri event
/// (`fs_ops://progress`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressEvent {
    pub kind: ProgressEventKind,
    pub name: String,
    pub index: u32,
    pub total: u32,
    /// R-44: `Some("canceled")` for a canceled item; also carries the
    /// ordinary failure code (`"not_found"`, `"conflict"`, `"io_error"`,
    /// `"invalid_path"`, `"overwrite_ancestor"`) for other `Fail` events.
    /// Always `None` for `Start`/`Done`.
    pub code: Option<String>,
}

fn item_display_name(item: &Path) -> String {
    item.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| item.to_string_lossy().into_owned())
}

/// Records one failed item in `summary` and emits its `Fail` event.
fn record_failure(
    summary: &mut OpSummary,
    on_event: &mut impl FnMut(ProgressEvent),
    item: &Path,
    name: String,
    index: u32,
    code: &str,
    message: String,
) {
    summary.failed += 1;
    summary.failures.push(Failure {
        path: item.to_path_buf(),
        code: code.to_string(),
        message,
    });
    on_event(ProgressEvent {
        kind: ProgressEventKind::Fail,
        name,
        index,
        total: summary.total,
        code: Some(code.to_string()),
    });
}

/// MUST-59 (R-37-2): why the existing `target` must not be overwritten by
/// `item`, if at all. Pure/non-destructive. `None` = an ordinary conflict.
fn overwrite_refusal(
    target: &Path,
    item: &Path,
    same_entity_result: Option<bool>,
) -> Option<(&'static str, &'static str)> {
    if same_entity_result.is_none() {
        return Some(("io_error", "cannot verify that the destination is a different item; not overwritten"));
    }
    match is_strictly_within(target, item) {
        Some(false) => None,
        Some(true) => Some((
            "overwrite_ancestor",
            "the source is inside the item to be overwritten; not overwritten",
        )),
        None => Some(("io_error", "cannot verify the overwrite target; not overwritten")),
    }
}

/// R-20~R-25/R-30~R-32/R-40~R-44: shared batch loop for both copy and move,
/// with per-item progress notification (`on_event`) and cooperative
/// cancellation (`should_cancel`, checked at each item boundary, R-43). For
/// each item: reject missing sources (`not_found`), exclude
/// same-item pastes (R-25, `skipped`, decided by canonical identity, R-37-1),
/// resolve destination-name conflicts (R-30~R-32, evaluated against real
/// filesystem state at the time each item is processed — never a
/// precomputed listing) using the per-item `resolver` (R-34-2), then perform
/// the transfer (overwrite = file replace + folder merge, R-31-2~R-31-7).
///
/// `run_transfer_batch`/`copy_batch`/`move_batch` call this with a constant
/// resolver, a no-op `on_event` and a `should_cancel` that always returns
/// `false`, so their behavior (and every pre-existing test in this module)
/// is unchanged.
fn run_transfer_batch_with_progress(
    kind: TransferKind,
    items: &[PathBuf],
    destination: &Path,
    resolver: impl Fn(&Path) -> ConflictResolution,
    mut should_cancel: impl FnMut() -> bool,
    mut on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    // R-24/AC-72: run whole-batch preflight checks (no items, destination
    // missing/not-a-directory, a source equal to or containing the
    // destination) before touching the filesystem. On rejection, every item
    // is reported as failed and none is attempted.
    //
    // `TransferPreflightError::NotFound` is deliberately excluded from this
    // whole-batch rejection: `preflight_transfer` flags it as soon as *any*
    // single item is missing, which would fail the entire batch — but
    // AC-68 requires that one missing source does not prevent the rest of
    // the batch from being processed. A missing individual source is
    // therefore left to the existing per-item `not_found` check below,
    // (SPEC-0300-0011 R-24 / R-19-7: paths taken from the OS clipboard are never
    // checked for existence up front, so they fail per item exactly like in-app ones;
    // AC-06-8 tests in `data_safety_tests.rs`),
    // which already implements the correct AC-68 semantics. The structural
    // checks above (destination validity, self-referential paths) are safe
    // to enforce batch-wide because they represent conditions where
    // attempting the transfer at all would be unsafe (e.g. infinite
    // recursion), not an ordinary per-item failure.
    //
    // No progress events are emitted here: as with R-25/`Skip` exclusions,
    // nothing in the batch was ever attempted.
    if let Err(err) = preflight::preflight_transfer(items, destination) {
        if err != TransferPreflightError::NotFound {
            let code = preflight_error_code(err);
            return OpSummary {
                total: items.len() as u32,
                failed: items.len() as u32,
                failures: items
                    .iter()
                    .map(|item| Failure {
                        path: item.clone(),
                        code: code.to_string(),
                        message: "rejected during preflight".to_string(),
                    })
                    .collect(),
                ..Default::default()
            };
        }
    }

    let total = items.len() as u32;
    let mut summary = OpSummary {
        total,
        ..Default::default()
    };

    for (i, item) in items.iter().enumerate() {
        let index = (i + 1) as u32;

        // R-43/R-44: check for a cancellation request at each item
        // boundary before touching this item. A canceled item is never
        // attempted (no `Start` event, no filesystem access) and is
        // reported with the dedicated `canceled` code so the UI can
        // distinguish it from an ordinary failure.
        if should_cancel() {
            record_failure(
                &mut summary,
                &mut on_event,
                item,
                item_display_name(item),
                index,
                "canceled",
                "canceled by user".to_string(),
            );
            continue;
        }

        let file_name = match item.file_name() {
            Some(name) => name,
            None => {
                let name = item_display_name(item);
                on_event(ProgressEvent {
                    kind: ProgressEventKind::Start,
                    name: name.clone(),
                    index,
                    total,
                    code: None,
                });
                record_failure(
                    &mut summary,
                    &mut on_event,
                    item,
                    name,
                    index,
                    "invalid_path",
                    "invalid source path".to_string(),
                );
                continue;
            }
        };
        let target = destination.join(file_name);
        let name = file_name.to_string_lossy().into_owned();
        let target_exists = entry_exists(&target);

        // R-25/R-37-1 (MUST-59): source and destination are the same entity
        // (decided by canonical identity, so letter case, junctions and
        // `\\?\` spellings all count) — exclude, do not attempt (no
        // progress event, see struct doc), do not count as a failure, and
        // above all never delete/overwrite it. `None` = identity could not
        // be determined; that only blocks the destructive `Overwrite` below.
        let same_entity_result = if target_exists { same_entity(item, &target) } else { Some(false) };
        if same_entity_result == Some(true) {
            summary.skipped += 1;
            continue;
        }

        let resolution = resolver(item);

        // R-31 "skip": excluded, not attempted (no progress event) — this
        // must be decided *before* the `Start` event below, unlike Reject/
        // Overwrite which only reveal themselves as a conflict once the
        // item is actually being processed.
        if target_exists && matches!(resolution, ConflictResolution::Skip) {
            summary.skipped += 1;
            continue;
        }

        on_event(ProgressEvent {
            kind: ProgressEventKind::Start,
            name: name.clone(),
            index,
            total,
            code: None,
        });

        if !entry_exists(item) {
            record_failure(&mut summary, &mut on_event, item, name, index, "not_found", "source not found".to_string());
            continue;
        }

        if target_exists {
            match resolution {
                ConflictResolution::Reject => {
                    // The UI sends `Reject` for an ancestor conflict: report
                    // that real reason (non-destructive check) before the
                    // generic `conflict`.
                    let (code, message) = overwrite_refusal(&target, item, same_entity_result)
                        .unwrap_or(("conflict", "destination already exists"));
                    record_failure(&mut summary, &mut on_event, item, name, index, code, message.to_string());
                    continue;
                }
                ConflictResolution::Skip => {
                    unreachable!("Skip is excluded above, before the Start event")
                }
                ConflictResolution::Overwrite => {
                    // MUST-59 (R-37-2): right before touching the existing
                    // destination item, refuse when the source lies beneath
                    // it (overwriting would destroy the source) — and when
                    // identity cannot be established, do not overwrite.
                    let refusal = overwrite_refusal(&target, item, same_entity_result);
                    if let Some((code, message)) = refusal {
                        record_failure(&mut summary, &mut on_event, item, name, index, code, message.to_string());
                        continue;
                    }

                    let mut errors = Vec::new();
                    overwrite_entry(kind, item, item.is_dir(), &target, &mut errors);
                    match errors.into_iter().next() {
                        None => {
                            summary.ok += 1;
                            on_event(ProgressEvent {
                                kind: ProgressEventKind::Done,
                                name,
                                index,
                                total,
                                code: None,
                            });
                        }
                        Some(first) => {
                            record_failure(&mut summary, &mut on_event, item, name, index, first.code, first.message);
                        }
                    }
                    continue;
                }
            }
        }

        match run_transfer_item(kind, item, &target) {
            Ok(()) => {
                summary.ok += 1;
                on_event(ProgressEvent {
                    kind: ProgressEventKind::Done,
                    name,
                    index,
                    total,
                    code: None,
                });
            }
            Err(item_error) => {
                record_failure(&mut summary, &mut on_event, item, name, index, item_error.code, item_error.message);
            }
        }
    }

    summary
}

/// R-21/R-40~R-44: batch copy into `destination` (recursive for
/// directories, R-20), emitting one `ProgressEvent` per item boundary and
/// honoring cooperative cancellation via `should_cancel`. `copy_batch` is a
/// thin wrapper over this that supplies a no-op `on_event` and a
/// `should_cancel` that never returns `true`.
pub fn copy_batch_with_progress(
    items: &[PathBuf],
    destination: &Path,
    resolution: ConflictResolution,
    should_cancel: impl FnMut() -> bool,
    on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    run_transfer_batch_with_progress(TransferKind::Copy, items, destination, |_| resolution, should_cancel, on_event)
}

/// R-21/R-23/R-40~R-44: batch move into `destination` — rename first,
/// falling back to recursive-copy-then-remove on failure (e.g. a
/// cross-device move) — emitting progress and honoring cancellation. See
/// `copy_batch_with_progress`.
pub fn move_batch_with_progress(
    items: &[PathBuf],
    destination: &Path,
    resolution: ConflictResolution,
    should_cancel: impl FnMut() -> bool,
    on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    run_transfer_batch_with_progress(TransferKind::Move, items, destination, |_| resolution, should_cancel, on_event)
}

/// R-34-2: like `copy_batch_with_progress`, but each top-level item gets its
/// own resolution from `resolver` (called once per item with the item path
/// exactly as given in `items`). `fs_ops::commands` builds `resolver` from
/// the IPC `itemResolutions` map via `resolve_item_resolution`.
pub fn copy_batch_with_resolver(
    items: &[PathBuf],
    destination: &Path,
    resolver: impl Fn(&Path) -> ConflictResolution,
    should_cancel: impl FnMut() -> bool,
    on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    run_transfer_batch_with_progress(TransferKind::Copy, items, destination, resolver, should_cancel, on_event)
}

/// R-34-2: the move counterpart of `copy_batch_with_resolver`.
pub fn move_batch_with_resolver(
    items: &[PathBuf],
    destination: &Path,
    resolver: impl Fn(&Path) -> ConflictResolution,
    should_cancel: impl FnMut() -> bool,
    on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    run_transfer_batch_with_progress(TransferKind::Move, items, destination, resolver, should_cancel, on_event)
}

/// R-21: batch copy into `destination` (recursive for directories, R-20).
pub fn copy_batch(items: &[PathBuf], destination: &Path, resolution: ConflictResolution) -> OpSummary {
    copy_batch_with_progress(items, destination, resolution, || false, |_| {})
}

/// R-21/R-23: batch move into `destination` — rename first, falling back to
/// recursive-copy-then-remove on failure (e.g. a cross-device move).
pub fn move_batch(items: &[PathBuf], destination: &Path, resolution: ConflictResolution) -> OpSummary {
    move_batch_with_progress(items, destination, resolution, || false, |_| {})
}

/// R-31-3 / R-37-2 / SPEC-0200-0001 §3.3.13-2. `Overwritable` = ordinary
/// top-level name conflict; `OverwriteAncestor` = the source lives beneath
/// the existing destination item (R-37-2) — or its identity could not be
/// established — so overwrite must not be offered (R-37-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    Overwritable,
    OverwriteAncestor,
}

/// One top-level conflict reported by `detect_conflicts` (`path` = the
/// source item, `name` = its file name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub path: PathBuf,
    pub name: String,
    pub kind: ConflictKind,
}

/// SPEC-0300-0011 R-31-3/R-32/R-37-1/R-37-2, SPEC-0200-0001 §3.3.7/§3.3.13-2/
/// §3.3.14-2. Side-effect-free detection of TOP-LEVEL name conflicts, in the
/// order of `items`. The same item (R-25, by canonical identity) is not a
/// conflict; a source beneath the existing destination item is
/// `OverwriteAncestor` (which takes precedence over a plain kind mismatch).
/// Items that do not exist are not reported (the transfer itself reports
/// `not_found`). Destination problems are reported with the preflight codes.
pub fn detect_conflicts(items: &[PathBuf], destination: &Path) -> Result<Vec<Conflict>, TransferPreflightError> {
    if items.is_empty() {
        return Err(TransferPreflightError::NoItems);
    }
    if destination.to_str().is_none() || items.iter().any(|item| item.to_str().is_none()) {
        return Err(TransferPreflightError::InvalidPath);
    }
    if !destination.exists() {
        return Err(TransferPreflightError::DestinationNotFound);
    }
    if !destination.is_dir() {
        return Err(TransferPreflightError::DestinationNotDir);
    }

    let mut conflicts = Vec::new();
    for item in items {
        let Some(file_name) = item.file_name() else { continue };
        let target = destination.join(file_name);
        if !entry_exists(&target) || !entry_exists(item) {
            continue;
        }
        let kind = match same_entity(item, &target) {
            Some(true) => continue,
            Some(false) => match is_strictly_within(&target, item) {
                Some(false) => ConflictKind::Overwritable,
                // Beneath the target, or undeterminable: never offer overwrite.
                Some(true) | None => ConflictKind::OverwriteAncestor,
            },
            None => ConflictKind::OverwriteAncestor,
        };
        conflicts.push(Conflict {
            path: item.clone(),
            name: file_name.to_string_lossy().into_owned(),
            kind,
        });
    }
    Ok(conflicts)
}

/// R-30-2/R-34-2, SPEC-0200-0001 §3.3.7: copy batch where each top-level
/// item may have its own resolution (keyed by the exact source path),
/// falling back to `default_resolution`. Keys that are not among `items` are
/// ignored.
pub fn copy_batch_with_item_resolutions(
    items: &[PathBuf],
    destination: &Path,
    default_resolution: ConflictResolution,
    item_resolutions: &std::collections::HashMap<PathBuf, ConflictResolution>,
) -> OpSummary {
    copy_batch_with_resolver(
        items,
        destination,
        |item| item_resolutions.get(item).copied().unwrap_or(default_resolution),
        || false,
        |_| {},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_file(dir: &std::path::Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).expect("write test file");
        path
    }

    // SPEC-0300-0011 AC-15 (R-24), integration regression: copy_batch must
    // reject a destination that is inside the source directory *before*
    // attempting anything (code-reviewer found this unwired on 2026-09-23,
    // causing an unbounded self-referential recursive copy that overflows
    // the stack). Zero side effects: the destination subtree gains nothing.
    #[test]
    fn ac15_copy_batch_rejects_destination_inside_source_with_zero_side_effects() {
        let dir = tempdir();
        let source_dir = dir.path().join("parent");
        let nested_destination = source_dir.join("child");
        fs::create_dir_all(&nested_destination).expect("create nested dirs");
        write_file(&source_dir, "top.txt", b"1");

        let summary = copy_batch(&[source_dir.clone()], &nested_destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failures[0].code, "destination_inside_source");
        // Confirms preflight ran before any copy work: the self-referential
        // copy that used to be attempted would have created `child/parent`.
        assert!(!nested_destination.join("parent").exists());
    }

    // SPEC-0300-0011 R-24 / AC-15 / AC-68, integration regression
    // (code-reviewer BLK-1, 2026-09-23): a missing item earlier in the batch
    // must not hide a structurally dangerous item later in the same batch.
    // Before the two-pass fix in `preflight_transfer`, a single combined
    // loop returned `NotFound` for the first (missing) item and never
    // reached the self-referential second item, so `run_transfer_batch`
    // treated the whole thing as an ordinary per-item failure and let the
    // dangerous copy through — reproducing the original stack-overflow
    // crash whenever a missing source happened to sort before it.
    #[test]
    fn ac15_destination_inside_source_is_still_caught_when_a_missing_item_sorts_first() {
        let dir = tempdir();
        let source_dir = dir.path().join("parent");
        let nested_destination = source_dir.join("child");
        fs::create_dir_all(&nested_destination).expect("create nested dirs");
        write_file(&source_dir, "top.txt", b"1");
        let missing_item = dir.path().join("missing.txt");

        let summary = copy_batch(
            &[missing_item.clone(), source_dir.clone()],
            &nested_destination,
            ConflictResolution::Reject,
        );

        // The whole batch is rejected up front (not: "missing.txt fails,
        // parent triggers a runaway recursive copy").
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 2);
        assert!(summary.failures.iter().all(|f| f.code == "destination_inside_source"));
        assert!(!nested_destination.join("parent").exists());
    }

    // SPEC-0300-0011 R-24, integration regression: an empty item list must
    // be rejected by copy_batch itself (not only by preflight_transfer in
    // isolation), with zero side effects.
    #[test]
    fn no_items_copy_batch_rejects_with_zero_side_effects() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");

        let summary = copy_batch(&[], &destination, ConflictResolution::Reject);

        assert_eq!(summary.total, 0);
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 0);
        let entries: Vec<_> = fs::read_dir(&destination).expect("read destination").collect();
        assert!(entries.is_empty());
    }

    // SPEC-0300-0011 AC-08 (R-01, R-20): copying a single file leaves the
    // original in place and creates a copy at the destination.
    #[test]
    fn ac08_copy_single_file_leaves_source_intact() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let source = write_file(dir.path(), "a.txt", b"hello");

        let summary = copy_batch(&[source.clone()], &destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 0);
        assert!(source.exists(), "source must still exist after copy");
        assert!(destination.join("a.txt").exists(), "copy must land at destination");
    }

    // SPEC-0300-0011 AC-09 (R-02, R-20): moving a single file removes the
    // original from its source location.
    #[test]
    fn ac09_move_single_file_removes_source() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let source = write_file(dir.path(), "a.txt", b"hello");

        let summary = move_batch(&[source.clone()], &destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 0);
        assert!(!source.exists(), "source must be gone after a successful move");
        assert!(destination.join("a.txt").exists(), "moved file must land at destination");
    }

    // SPEC-0300-0011 AC-10 (R-20): copying a directory recursively copies
    // its full contents, including nested subdirectories.
    #[test]
    fn ac10_copy_directory_recursively_copies_contents() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let source_dir = dir.path().join("src_dir");
        fs::create_dir_all(source_dir.join("nested")).expect("create nested dir");
        fs::write(source_dir.join("top.txt"), b"1").expect("write top.txt");
        fs::write(source_dir.join("nested").join("deep.txt"), b"2").expect("write deep.txt");

        let summary = copy_batch(&[source_dir.clone()], &destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 1);
        assert!(destination.join("src_dir").join("top.txt").exists());
        assert!(destination.join("src_dir").join("nested").join("deep.txt").exists());
        // Recursive copy must not remove the original.
        assert!(source_dir.join("nested").join("deep.txt").exists());
    }

    // SPEC-0300-0011 AC-11 (R-20): multiple selected items are processed in
    // a single batch call.
    #[test]
    fn ac11_batch_processes_multiple_items_in_one_call() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let a = write_file(dir.path(), "a.txt", b"1");
        let b = write_file(dir.path(), "b.txt", b"2");

        let summary = copy_batch(&[a, b], &destination, ConflictResolution::Reject);

        assert_eq!(summary.total, 2);
        assert_eq!(summary.ok, 2);
        assert!(destination.join("a.txt").exists());
        assert!(destination.join("b.txt").exists());
    }

    // SPEC-0300-0011 AC-12 (R-23, main's bug fix): when the OS rename fails
    // but the copy+remove fallback succeeds, the item must be reported as a
    // success, not counted against `failed`. This is the crux of `main`'s
    // bug (it returned the original rename error even on fallback success).
    #[test]
    fn ac12_r23_fallback_success_is_reported_as_success() {
        let rename_result: Result<(), String> = Err("cross-device link".to_string());
        let fallback_result: Option<Result<(), String>> = Some(Ok(()));

        let outcome = resolve_move_item_outcome(rename_result, fallback_result);
        assert_eq!(outcome, Ok(()), "R-23: a successful fallback must be reported as Ok, not the original rename error");
    }

    // SPEC-0300-0011 AC-13 (R-23): when the fallback itself also fails
    // (either the copy or the removal step), the item is reported as a
    // failure.
    #[test]
    fn ac13_r23_fallback_failure_is_reported_as_failure() {
        let rename_result: Result<(), String> = Err("cross-device link".to_string());
        let fallback_result: Option<Result<(), String>> = Some(Err("disk full".to_string()));

        let outcome = resolve_move_item_outcome(rename_result, fallback_result);
        assert!(outcome.is_err(), "a failed fallback must be reported as a failure");
    }

    // A plain successful rename (no fallback attempted) is reported as a
    // success (baseline sanity check for resolve_move_item_outcome).
    #[test]
    fn plain_rename_success_needs_no_fallback() {
        let outcome = resolve_move_item_outcome(Ok(()), None);
        assert_eq!(outcome, Ok(()));
    }

    // SPEC-0300-0011 AC-14 (R-25): a paste where source and destination
    // directory are identical is excluded from processing (not attempted,
    // not counted as a failure) and tracked as skipped so the UI can report
    // an exclusion count.
    #[test]
    fn ac14_same_directory_paste_is_excluded_not_counted_as_failure() {
        let dir = tempdir();
        let source = write_file(dir.path(), "a.txt", b"1");

        // Pasting into the very directory that already contains the item.
        let summary = copy_batch(&[source], dir.path(), ConflictResolution::Reject);

        assert_eq!(summary.failed, 0, "R-25 exclusion must not be counted as a failure");
        assert_eq!(summary.skipped, 1, "R-25 exclusion must be reflected in the skipped/excluded count");
    }

    // SPEC-0300-0011 AC-18 (R-22): the batch result exposes ok/failed/total
    // and a failures list.
    #[test]
    fn ac18_batch_result_exposes_ok_failed_total_and_failures() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let existing_source = write_file(dir.path(), "a.txt", b"1");
        let missing_source = dir.path().join("missing.txt");

        let summary = copy_batch(
            &[existing_source, missing_source.clone()],
            &destination,
            ConflictResolution::Reject,
        );

        assert_eq!(summary.total, 2);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failures.len(), 1);
        assert_eq!(summary.failures[0].path, missing_source);
        assert_eq!(summary.failures[0].code, "not_found");
    }

    // SPEC-0300-0011 AC-29 (R-30, R-30-1, R-30-2): calling the batch
    // transfer directly (the non-UI path) with `ConflictResolution::Reject`
    // and a pre-existing destination file must reject that item with the
    // `conflict` error code — never silently overwrite it.
    #[test]
    fn ac29_non_ui_path_without_a_resolution_rejects_conflicts() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        fs::write(destination.join("a.txt"), b"existing").expect("seed existing destination file");
        let source = write_file(dir.path(), "a.txt", b"new");

        let summary = copy_batch(&[source], &destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failures[0].code, "conflict");
        assert_eq!(
            fs::read(destination.join("a.txt")).expect("read destination"),
            b"existing",
            "the pre-existing destination file must not be overwritten"
        );
    }

    // SPEC-0300-0011 AC-29-1 (R-32, R-32-1): conflict detection is based on
    // the filesystem's actual current state at call time, not on any
    // pre-computed listing — a file that appears at the destination "just
    // before" the call is still detected.
    #[test]
    fn ac29_1_conflict_is_detected_via_filesystem_state_not_a_stale_listing() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let source = write_file(dir.path(), "a.txt", b"new");
        // Simulates a conflicting file that another process created at the
        // destination right before this call — there is no separate
        // "known entries" listing passed into copy_batch for it to be
        // stale against; detection must be against the real filesystem.
        fs::write(destination.join("a.txt"), b"just created by someone else").expect("seed conflict");

        let summary = copy_batch(&[source], &destination, ConflictResolution::Reject);

        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failures[0].code, "conflict");
    }

    // SPEC-0300-0011 AC-20 (R-31 "overwrite"): given an explicit Overwrite
    // resolution, a conflicting destination file is replaced with the
    // source's content.
    #[test]
    fn ac20_overwrite_resolution_replaces_existing_destination_content() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        fs::write(destination.join("a.txt"), b"old").expect("seed existing destination file");
        let source = write_file(dir.path(), "a.txt", b"new");

        let summary = copy_batch(&[source], &destination, ConflictResolution::Overwrite);

        assert_eq!(summary.ok, 1);
        assert_eq!(fs::read(destination.join("a.txt")).expect("read destination"), b"new");
    }

    // SPEC-0300-0011 AC-21 (R-31 "skip"): given an explicit Skip
    // resolution, a conflicting item is left untouched, and unrelated
    // non-conflicting items in the same batch still complete.
    #[test]
    fn ac21_skip_resolution_leaves_existing_destination_unchanged_and_continues_others() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        fs::write(destination.join("a.txt"), b"old").expect("seed existing destination file");
        let conflicting_source = write_file(dir.path(), "a.txt", b"new");
        let clean_source = write_file(dir.path(), "b.txt", b"2");

        let summary = copy_batch(
            &[conflicting_source, clean_source],
            &destination,
            ConflictResolution::Skip,
        );

        assert_eq!(fs::read(destination.join("a.txt")).expect("read destination"), b"old");
        assert!(destination.join("b.txt").exists(), "the non-conflicting item must still be processed");
        assert_eq!(summary.failed, 0, "a skip is not a failure");
    }

    // SPEC-0300-0011 AC-68 (R-80, R-86): when one item in a batch fails
    // (here: a missing source, a stand-in for any single-item failure), the
    // remaining items are still processed to completion.
    #[test]
    fn ac68_one_failing_item_does_not_prevent_the_rest_of_the_batch() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let missing_source = dir.path().join("missing.txt");
        let good_source = write_file(dir.path(), "good.txt", b"1");

        let summary = copy_batch(&[missing_source, good_source], &destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 1);
        assert!(destination.join("good.txt").exists());
    }

    // --- SPEC-0300-0011 R-40~R-45 (ISS-0052): progress events and cancellation ---

    // AC-31/R-40/R-41: a successful item emits a `Start` event (announcing
    // the name and current/total position) followed by a `Done` event.
    #[test]
    fn ac31_successful_item_emits_start_then_done_with_name_and_position() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let source = write_file(dir.path(), "a.txt", b"1");

        let mut events: Vec<ProgressEvent> = Vec::new();
        let summary = copy_batch_with_progress(
            &[source],
            &destination,
            ConflictResolution::Reject,
            || false,
            |event| events.push(event),
        );

        assert_eq!(summary.ok, 1);
        assert_eq!(events.len(), 2, "expected exactly one Start and one Done event");
        assert_eq!(events[0].kind, ProgressEventKind::Start);
        assert_eq!(events[0].name, "a.txt");
        assert_eq!(events[0].index, 1);
        assert_eq!(events[0].total, 1);
        assert_eq!(events[1].kind, ProgressEventKind::Done);
        assert_eq!(events[1].name, "a.txt");
    }

    // R-40/R-44: a failing item (here: not_found) still gets a `Start` event
    // (it was attempted) followed by a `Fail` event carrying the failure
    // code, so the UI can show "processing <name> ... failed".
    #[test]
    fn r40_failing_item_emits_start_then_fail_with_code() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let missing_source = dir.path().join("missing.txt");

        let mut events: Vec<ProgressEvent> = Vec::new();
        let summary = copy_batch_with_progress(
            &[missing_source],
            &destination,
            ConflictResolution::Reject,
            || false,
            |event| events.push(event),
        );

        assert_eq!(summary.failed, 1);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, ProgressEventKind::Start);
        assert_eq!(events[1].kind, ProgressEventKind::Fail);
        assert_eq!(events[1].code.as_deref(), Some("not_found"));
    }

    // R-25/R-31: items excluded from processing without ever being
    // attempted (same-directory paste, or an explicit Skip resolution)
    // must not emit any progress event at all.
    #[test]
    fn r25_r31_excluded_items_emit_no_progress_event() {
        let dir = tempdir();
        let source = write_file(dir.path(), "a.txt", b"1");

        let mut events: Vec<ProgressEvent> = Vec::new();
        // Pasting into the very directory that already contains the item
        // (R-25 same-directory exclusion).
        let summary = copy_batch_with_progress(
            &[source],
            dir.path(),
            ConflictResolution::Reject,
            || false,
            |event| events.push(event),
        );

        assert_eq!(summary.skipped, 1);
        assert!(events.is_empty(), "an excluded item must emit no Start/Done/Fail event");
    }

    // R-43/R-44 (the scenario from the task description): given a 4-item
    // batch, if the cancellation flag is raised right after the 2nd item
    // finishes, the 3rd and 4th items must be reported as `canceled` and
    // must never actually be touched on disk (no Start event, no file
    // operation performed).
    #[test]
    fn r43_r44_cancel_after_second_item_marks_the_rest_canceled_without_touching_them() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let a = write_file(dir.path(), "a.txt", b"1");
        let b = write_file(dir.path(), "b.txt", b"2");
        let c = write_file(dir.path(), "c.txt", b"3");
        let d = write_file(dir.path(), "d.txt", b"4");

        let cancel_flag = std::cell::Cell::new(false);
        let mut events: Vec<ProgressEvent> = Vec::new();

        let summary = copy_batch_with_progress(
            &[a, b, c.clone(), d.clone()],
            &destination,
            ConflictResolution::Reject,
            || cancel_flag.get(),
            |event| {
                if event.kind == ProgressEventKind::Done && event.index == 2 {
                    // Simulates the user clicking "cancel" (or pressing
                    // Escape, R-42-2) while the 2nd item is wrapping up.
                    cancel_flag.set(true);
                }
                events.push(event);
            },
        );

        assert_eq!(summary.ok, 2, "the first two items must have completed normally");
        assert_eq!(summary.failed, 2, "the remaining two items must be reported as failed (canceled)");
        assert!(
            summary
                .failures
                .iter()
                .filter(|f| f.path == c || f.path == d)
                .all(|f| f.code == "canceled"),
            "items reached after cancellation must use the dedicated 'canceled' code (R-44)"
        );

        // R-43: canceled items must never be touched — no Start event, and
        // no actual filesystem side effect.
        let fail_events: Vec<&ProgressEvent> = events.iter().filter(|e| e.code.as_deref() == Some("canceled")).collect();
        assert_eq!(fail_events.len(), 2);
        assert!(fail_events.iter().all(|e| e.kind == ProgressEventKind::Fail));
        assert!(
            !events.iter().any(|e| e.kind == ProgressEventKind::Start && (e.name == "c.txt" || e.name == "d.txt")),
            "a canceled item must never emit a Start event"
        );
        assert!(c.exists(), "a canceled item's source must be left untouched");
        assert!(d.exists(), "a canceled item's source must be left untouched");
        assert!(!destination.join("c.txt").exists(), "a canceled item must not be copied to the destination");
        assert!(!destination.join("d.txt").exists(), "a canceled item must not be copied to the destination");
    }

    // R-43: cancellation is checked at the very first item boundary too —
    // if the flag is already raised before anything starts, the whole
    // batch is canceled and zero filesystem operations are performed.
    #[test]
    fn r43_cancellation_before_the_first_item_cancels_the_whole_batch() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let a = write_file(dir.path(), "a.txt", b"1");

        let mut events: Vec<ProgressEvent> = Vec::new();
        let summary = copy_batch_with_progress(
            &[a.clone()],
            &destination,
            ConflictResolution::Reject,
            || true,
            |event| events.push(event),
        );

        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failures[0].code, "canceled");
        assert_eq!(events.len(), 1, "only the Fail(canceled) event, no Start");
        assert_eq!(events[0].kind, ProgressEventKind::Fail);
        assert!(a.exists());
        assert!(!destination.join("a.txt").exists());
    }

    // R-45 (sanity check at this layer): copy_batch/move_batch (the
    // pre-existing, no-progress entry points relied upon by every test
    // above) must remain fully functional no-op-callback wrappers — this
    // is implicitly exercised by every other test in this module still
    // passing unchanged, but this test pins the two entry points down
    // explicitly against a should_cancel that is never polled to `true`.
    #[test]
    fn copy_batch_and_move_batch_are_unaffected_by_the_progress_plumbing() {
        let dir = tempdir();
        let destination = dir.path().join("dest");
        fs::create_dir(&destination).expect("create destination");
        let source = write_file(dir.path(), "a.txt", b"1");

        let summary = copy_batch(&[source], &destination, ConflictResolution::Reject);

        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 0);
    }
}
