//! The inner layer of the `archive_*` commands (SPEC-0200-0001 §3.12.3 #11..#14 / §3.12.5;
//! SPEC-0300-0011 第9版 R-89 / R-89-6-1 / T-10 / T-13 / T-14; SPEC-0300-0014 第5版 R-322-1/3, R-341-1/2).
//!
//! No Tauri types: `commands.rs` keeps the cancel registration, the progress events and the DTO
//! conversion and calls these (`commands.rs -> guarded.rs` only, never back, SHOULD-187).
//!
//! Body order (create): `items.is_empty()` -> `no_items` (SHOULD-188) -> absolute check of `items` and
//! `base_dir` -> `/` to `\` on the output INPUT (T-14) -> `resolve_output_path` -> absolute check of the
//! resolved path (T-13) -> [E2E scope checks, create / extract only] -> the real work.
//! Extract / detect: absolute check of `zip_path` and `base_dir` ALWAYS (T-10) -> separators ->
//! `join_destination` -> absolute check of the resolved path -> [scope checks, extract only] -> work.
//! The SAME replaced strings are passed on to `create_zip` / `build_plan` / `extract`, which resolve again.
//! Refusals are `ArchiveErrorDto { code: "invalid_path", entry_path: None }`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::create::{
    check_create, create_zip, resolve_output_path, CheckedCreate, CreateHooks, CreateResult,
};
use super::error::{ArchiveError, ArchiveErrorCode as Code, ArchiveErrorDto};
use super::extract::{extract, ExtractHooks, ExtractResult, Resolution};
use super::extract_plan::{build_plan, join_destination, ExtractEnv, Plan};
use super::path_rules::{normalize_input_separators, strip_verbatim_prefix};
use crate::shared::abs_path;
use crate::shared::e2e_scope::{self, ScopeViolation, VIOLATION_CODE};

/// Debug-only E2E scope refusal (SPEC-0400-0001 R-10-3): rejected before anything is read or written.
pub(super) fn scope_error(violation: ScopeViolation) -> ArchiveErrorDto {
    ArchiveErrorDto {
        code: VIOLATION_CODE.to_string(),
        message: violation.0,
        entry_path: None,
    }
}

/// Path text for the UI: no `\?\` prefix (SHOULD-90).
pub fn display(path: &Path) -> String {
    strip_verbatim_prefix(&path.to_string_lossy())
}

pub(super) fn to_paths(items: &[String]) -> Vec<PathBuf> {
    items.iter().map(PathBuf::from).collect()
}

/// R-89-2: `invalid_path`, naming the refused path in the message.
fn invalid_path(path: &str) -> ArchiveErrorDto {
    ArchiveError::new(Code::InvalidPath, format!("not an absolute path: {path}")).into()
}

pub fn check_create_guarded(
    items: &[String],
    output_input: &str,
    base_dir: &str,
) -> Result<CheckedCreate, ArchiveErrorDto> {
    if items.is_empty() {
        return Err(ArchiveError::new(Code::NoItems, "nothing is selected").into());
    }
    if let Some(bad) = abs_path::first_relative(items.iter().map(String::as_str).chain([base_dir]))
    {
        return Err(invalid_path(bad));
    }
    let output_input = normalize_input_separators(output_input);
    let resolved = resolve_output_path(&output_input, Path::new(base_dir))?;
    // T-13: the RESOLVED path must itself be absolute (a device path or a share-less UNC).
    if !abs_path::is_absolute_path_buf(&resolved) {
        return Err(invalid_path(&resolved.to_string_lossy()));
    }
    Ok(check_create(
        &to_paths(items),
        &output_input,
        Path::new(base_dir),
    )?)
}

pub fn create_guarded(
    items: &[String],
    output_input: &str,
    base_dir: &str,
    overwrite: bool,
    hooks: &CreateHooks,
) -> Result<CreateResult, ArchiveErrorDto> {
    if items.is_empty() {
        return Err(ArchiveError::new(Code::NoItems, "nothing is selected").into());
    }
    if let Some(bad) = abs_path::first_relative(items.iter().map(String::as_str).chain([base_dir]))
    {
        return Err(invalid_path(bad));
    }
    let output_input = normalize_input_separators(output_input);
    let resolved = resolve_output_path(&output_input, Path::new(base_dir))?;
    // T-13: the RESOLVED path must itself be absolute (a device path or a share-less UNC).
    if !abs_path::is_absolute_path_buf(&resolved) {
        return Err(invalid_path(&resolved.to_string_lossy()));
    }
    // R-10: output folder = container; items and base_dir are read sources.
    e2e_scope::check_container(resolved.parent().unwrap_or(Path::new(""))).map_err(scope_error)?;
    items
        .iter()
        .try_for_each(|item| e2e_scope::check_source(Path::new(item)))
        .map_err(scope_error)?;
    e2e_scope::check_source(Path::new(base_dir)).map_err(scope_error)?;
    Ok(create_zip(
        &to_paths(items),
        &output_input,
        Path::new(base_dir),
        overwrite,
        hooks,
    )?)
}

pub fn detect_conflicts_guarded(
    zip_path: &str,
    dest_input: &str,
    base_dir: &str,
    env: &ExtractEnv,
) -> Result<Plan, ArchiveErrorDto> {
    if let Some(bad) = abs_path::first_relative([zip_path, base_dir]) {
        return Err(invalid_path(bad));
    }
    let dest_input = normalize_input_separators(dest_input);
    let joined = join_destination(Path::new(&dest_input), Path::new(base_dir))?;
    // T-13: the RESOLVED path must itself be absolute (a device path or a share-less UNC).
    if !abs_path::is_absolute_path_buf(&joined) {
        return Err(invalid_path(&joined.to_string_lossy()));
    }
    Ok(build_plan(
        Path::new(zip_path),
        Path::new(&dest_input),
        Path::new(base_dir),
        env,
    )?)
}

pub fn extract_guarded(
    zip_path: &str,
    dest_input: &str,
    base_dir: &str,
    env: &ExtractEnv,
    resolutions: &HashMap<String, Resolution>,
    hooks: &ExtractHooks,
) -> Result<ExtractResult, ArchiveErrorDto> {
    if let Some(bad) = abs_path::first_relative([zip_path, base_dir]) {
        return Err(invalid_path(bad));
    }
    let dest_input = normalize_input_separators(dest_input);
    let joined = join_destination(Path::new(&dest_input), Path::new(base_dir))?;
    // T-13: the RESOLVED path must itself be absolute (a device path or a share-less UNC).
    if !abs_path::is_absolute_path_buf(&joined) {
        return Err(invalid_path(&joined.to_string_lossy()));
    }
    // R-10: destination = container; the ZIP is a read source.
    e2e_scope::check_container(&joined).map_err(scope_error)?;
    e2e_scope::check_source(Path::new(zip_path)).map_err(scope_error)?;
    Ok(extract(
        Path::new(zip_path),
        Path::new(&dest_input),
        Path::new(base_dir),
        env,
        resolutions,
        hooks,
    )?)
}
