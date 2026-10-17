//! The inner layer of the `fs_ops` commands (SPEC-0200-0001 §3.12.3 #3..#9; SPEC-0300-0011 第9版
//! R-89 / R-89-6-1).
//!
//! No Tauri types: `commands.rs` keeps the cancel registration, the progress events and the DTO
//! conversion and calls these. Every function runs `abs_path::first_relative(...)` -> E2E scope
//! checks -> the real work, in that order (R-89-4).
//!
//! - a non-absolute argument (any `items` / `destination` / `paths` / `path` / `parent` element) refuses
//!   the WHOLE call; nothing is read, written, moved, deleted or emitted;
//! - batches (copy / move / delete) return `OpSummary { ok: 0, failed: n, total: n, skipped: 0,
//!   failures: one per item (path = the item as given), code "invalid_path" }`, `n = items.len()`
//!   (`destination` only bad -> still every item fails; empty `items` -> all zeros);
//! - `detect_conflicts` / `rename` / `create` return the bare code `Err("invalid_path")`.

use std::path::{Path, PathBuf};

use super::create;
use super::delete;
use super::preflight::MutatePreflightError;
use super::rename;
use super::transfer::{self, Conflict, ConflictResolution, Failure, OpSummary, ProgressEvent};
use crate::shared::abs_path;
use crate::shared::e2e_scope::{self, ScopeViolation, VIOLATION_CODE};

fn to_path_bufs(items: &[String]) -> Vec<PathBuf> {
    items.iter().map(PathBuf::from).collect()
}

/// Every item fails with `code` and nothing is touched (R-89-1 / T-04; also the debug-only E2E scope
/// refusal, SPEC-0400-0001 R-10-3).
fn refuse_all(paths: &[String], code: &str, message: &str) -> OpSummary {
    let count = paths.len() as u32;
    OpSummary {
        ok: 0,
        failed: count,
        total: count,
        skipped: 0,
        failures: paths
            .iter()
            .map(|path| Failure { path: PathBuf::from(path), code: code.to_string(), message: message.to_string() })
            .collect(),
    }
}

fn invalid_path_message(bad: &str) -> String {
    format!("not an absolute path: {bad}")
}

fn scope_refusal(paths: &[String], violation: &ScopeViolation) -> OpSummary {
    refuse_all(paths, VIOLATION_CODE, &violation.0)
}

fn mutate_error_code(err: MutatePreflightError) -> String {
    match err {
        MutatePreflightError::Required => "invalid_name".to_string(),
        MutatePreflightError::InvalidName => "invalid_name".to_string(),
        MutatePreflightError::AlreadyExists => "already_exists".to_string(),
        MutatePreflightError::ParentNotFound => "parent_not_found".to_string(),
        MutatePreflightError::InvalidKind => "invalid_kind".to_string(),
    }
}

pub fn copy_guarded(
    items: &[String],
    destination: &str,
    resolver: impl Fn(&Path) -> ConflictResolution,
    should_cancel: impl FnMut() -> bool,
    on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    if let Some(bad) = abs_path::first_relative(items.iter().map(String::as_str).chain([destination])) {
        return refuse_all(items, abs_path::INVALID_PATH_CODE, &invalid_path_message(bad));
    }
    // R-10 (debug builds with a scope only): destination = container, items = read sources.
    let scope = e2e_scope::check_container(Path::new(destination))
        .and_then(|_| items.iter().try_for_each(|item| e2e_scope::check_source(Path::new(item))));
    if let Err(violation) = scope {
        return scope_refusal(items, &violation);
    }
    transfer::copy_batch_with_resolver(&to_path_bufs(items), Path::new(destination), resolver, should_cancel, on_event)
}

pub fn move_guarded(
    items: &[String],
    destination: &str,
    resolver: impl Fn(&Path) -> ConflictResolution,
    should_cancel: impl FnMut() -> bool,
    on_event: impl FnMut(ProgressEvent),
) -> OpSummary {
    if let Some(bad) = abs_path::first_relative(items.iter().map(String::as_str).chain([destination])) {
        return refuse_all(items, abs_path::INVALID_PATH_CODE, &invalid_path_message(bad));
    }
    // R-10: destination = container; items are moved (entry rule) and read (source rule).
    let scope = e2e_scope::check_container(Path::new(destination)).and_then(|_| {
        items.iter().try_for_each(|item| {
            e2e_scope::check_entry(Path::new(item)).and_then(|_| e2e_scope::check_source(Path::new(item)))
        })
    });
    if let Err(violation) = scope {
        return scope_refusal(items, &violation);
    }
    transfer::move_batch_with_resolver(&to_path_bufs(items), Path::new(destination), resolver, should_cancel, on_event)
}

pub fn detect_conflicts_guarded(items: &[String], destination: &str) -> Result<Vec<Conflict>, String> {
    if abs_path::first_relative(items.iter().map(String::as_str).chain([destination])).is_some() {
        return Err(abs_path::INVALID_PATH_CODE.to_string());
    }
    transfer::detect_conflicts(&to_path_bufs(items), Path::new(destination))
        .map_err(|err| transfer::preflight_error_code(err).to_string())
}

pub fn delete_trash_guarded(paths: &[String]) -> OpSummary {
    if let Some(bad) = abs_path::first_relative(paths.iter().map(String::as_str)) {
        return refuse_all(paths, abs_path::INVALID_PATH_CODE, &invalid_path_message(bad));
    }
    if let Err(violation) = paths.iter().try_for_each(|p| e2e_scope::check_entry(Path::new(p))) {
        return scope_refusal(paths, &violation);
    }
    delete::delete_to_trash(&to_path_bufs(paths))
}

pub fn delete_permanently_guarded(paths: &[String]) -> OpSummary {
    if let Some(bad) = abs_path::first_relative(paths.iter().map(String::as_str)) {
        return refuse_all(paths, abs_path::INVALID_PATH_CODE, &invalid_path_message(bad));
    }
    if let Err(violation) = paths.iter().try_for_each(|p| e2e_scope::check_entry(Path::new(p))) {
        return scope_refusal(paths, &violation);
    }
    delete::delete_permanently(&to_path_bufs(paths))
}

pub fn rename_guarded(path: &str, new_name: &str) -> Result<String, String> {
    if abs_path::first_relative([path]).is_some() {
        return Err(abs_path::INVALID_PATH_CODE.to_string());
    }
    // R-10: the entry itself, and the folder it stays in (the new name's parent).
    let scope = e2e_scope::check_entry(Path::new(path))
        .and_then(|_| e2e_scope::check_container(Path::new(path).parent().unwrap_or(Path::new(""))));
    scope.map_err(|violation| violation.to_command_error())?;
    rename::rename_entry(Path::new(path), new_name)
        .map(|new_path| new_path.to_string_lossy().into_owned())
        .map_err(mutate_error_code)
}

pub fn create_guarded(parent: &str, name: &str, kind: &str) -> Result<(), String> {
    if abs_path::first_relative([parent]).is_some() {
        return Err(abs_path::INVALID_PATH_CODE.to_string());
    }
    e2e_scope::check_container(Path::new(parent)).map_err(|violation| violation.to_command_error())?;
    create::create_entry(Path::new(parent), name, kind).map_err(mutate_error_code)
}

fn resolution_from_str(value: &str) -> ConflictResolution {
    match value {
        "overwrite" => ConflictResolution::Overwrite,
        "skip" => ConflictResolution::Skip,
        _ => ConflictResolution::Reject,
    }
}

pub fn resolve_item_resolution(
    item: &str,
    default: ConflictResolution,
    item_resolutions: Option<& std::collections::HashMap<String, String>>,
) -> ConflictResolution {
    match item_resolutions.and_then(|map| map.get(item)) {
        Some(value) => resolution_from_str(value),
        None => default,
    }
}

