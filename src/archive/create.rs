//! `archive/create.rs` (SPEC-0300-0014 §4: R-321~R-329, R-322-1, R-323,
//! R-324-1, R-325-1, R-326-3, R-326-4, R-327-1~R-327-11, R-328-x, R-382;
//! SPEC-0200-0001 §3.4.3, §3.4.13-1 / -2).
//!
//! Framework-agnostic: the Tauri layer is `archive/commands.rs`.
//! Tests: `archive/create_tests.rs`.

use super::error::{classify_io_error, ArchiveError, ArchiveErrorCode as Code};
use super::path_rules::{os_name_to_utf8, validate_output_file_name};
use super::progress::ProgressEvent;
use super::temp::TempFile;
use super::win_fs::move_no_replace;
use super::zip_time::system_time_to_zip;
use crate::shared::identity::{normalize_allowing_missing, same_entity};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// Copy block size; also the cancellation polling granularity (R-382).
const BLOCK: usize = 64 * 1024;
/// R-327-10: the ZIP format stores an entry name length in 16 bits.
const MAX_NAME_BYTES: usize = 65_535;

/// Result of the side-effect-free pre-check (`archive_check_create`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedCreate {
    /// Resolved output path (relative -> under `base_dir`, `.zip` appended).
    pub output_path: PathBuf,
    /// `true` when an existing FILE would be replaced (R-326).
    pub overwrite_required: bool,
}

fn canceled() -> ArchiveError {
    ArchiveError::new(Code::Canceled, "the operation was canceled")
}

fn io_failure(err: &std::io::Error) -> ArchiveError {
    ArchiveError::new(classify_io_error(err), err.to_string())
}

fn zip_failure(err: zip::result::ZipError) -> ArchiveError {
    match err {
        zip::result::ZipError::Io(e) => io_failure(&e),
        other => ArchiveError::new(
            Code::IoError,
            format!("cannot write the ZIP archive: {other}"),
        ),
    }
}

/// R-322-1 / R-323 / R-324 (pure: no filesystem access): resolves the user
/// input. Blank -> `invalid_name`; drive-relative (`C:x`) / root-relative
/// (`\x`) -> `invalid_path`; `base_dir` not absolute -> `invalid_path`;
/// a relative input is joined to `base_dir`; the file name is validated
/// (`invalid_name`); `.zip` is appended unless the name already ends with it
/// (case-insensitive).
pub fn resolve_output_path(output_path: &str, base_dir: &Path) -> Result<PathBuf, ArchiveError> {
    if output_path.trim().is_empty() {
        return Err(ArchiveError::new(
            Code::InvalidName,
            "the output name is empty",
        ));
    }
    if !base_dir.is_absolute() {
        return Err(ArchiveError::new(
            Code::InvalidPath,
            "the base directory is not an absolute path",
        ));
    }
    if output_path.ends_with('\\') || output_path.ends_with('/') {
        return Err(ArchiveError::new(
            Code::InvalidName,
            "the output path has no file name",
        ));
    }
    let given = Path::new(output_path);
    let has_prefix = matches!(given.components().next(), Some(Component::Prefix(_)));
    let full = if given.is_absolute() {
        given.to_path_buf()
    } else if given.has_root() || has_prefix {
        return Err(ArchiveError::new(
            Code::InvalidPath,
            "a drive-relative or root-relative output path is ambiguous",
        ));
    } else {
        base_dir.join(given)
    };
    let name = full.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        ArchiveError::new(Code::InvalidName, "the output path has no usable file name")
    })?;
    validate_output_file_name(name)
        .map_err(|code| ArchiveError::new(code, "the output file name is not allowed"))?;
    if name.to_lowercase().ends_with(".zip") {
        Ok(full)
    } else {
        let with_extension = format!("{name}.zip");
        Ok(full.with_file_name(with_extension))
    }
}

/// The name an item gets inside the archive (R-327-1).
fn item_name(item: &Path) -> Result<String, ArchiveError> {
    let name = item.file_name().ok_or_else(|| {
        ArchiveError::new(
            Code::InvalidPath,
            "the item has no name to store in the archive",
        )
    })?;
    os_name_to_utf8(name)
        .map_err(|code| ArchiveError::new(code, "the item name is not valid Unicode"))
}

/// The full pre-check with NO side effect. Order (SPEC-0200-0001 §3.4.3):
/// `no_items` -> `not_found` (an item) -> `invalid_path` (an item with no
/// archive name such as `C:\`; an item name with an unpaired surrogate) ->
/// `invalid_name` / `invalid_path` (resolve_output_path) -> an existing link at
/// the output path (any target state) `invalid_path` -> `destination_not_found`
/// (parent missing, R-324-1) -> `destination_inside_source` (R-325-1; also when
/// identity cannot be established) -> `already_exists` (existing directory) ->
/// existing file: `overwrite_required = true`.
pub fn check_create(
    items: &[PathBuf],
    output_path: &str,
    base_dir: &Path,
) -> Result<CheckedCreate, ArchiveError> {
    if items.is_empty() {
        return Err(ArchiveError::new(Code::NoItems, "nothing is selected"));
    }
    for item in items {
        if let Err(e) = fs::symlink_metadata(item) {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                Code::NotFound
            } else {
                classify_io_error(&e)
            };
            return Err(ArchiveError::new(
                code,
                format!("cannot access `{}`: {e}", item.display()),
            ));
        }
    }
    for item in items {
        item_name(item)?;
    }
    let output = resolve_output_path(output_path, base_dir)?;
    let existing = fs::symlink_metadata(&output);
    if let Ok(meta) = &existing {
        if meta.file_type().is_symlink() {
            return Err(ArchiveError::new(
                Code::InvalidPath,
                "the output path is a link",
            ));
        }
    }
    match output.parent().map(fs::metadata) {
        Some(Ok(meta)) if meta.is_dir() => {}
        _ => {
            return Err(ArchiveError::new(
                Code::DestinationNotFound,
                "the output folder does not exist",
            ))
        }
    }
    check_not_inside_sources(items, &output, existing.is_ok())?;
    match existing {
        Ok(meta) if meta.is_dir() => Err(ArchiveError::new(
            Code::AlreadyExists,
            "a folder with the output name exists",
        )),
        Ok(_) => Ok(CheckedCreate {
            output_path: output,
            overwrite_required: true,
        }),
        Err(_) => Ok(CheckedCreate {
            output_path: output,
            overwrite_required: false,
        }),
    }
}

/// R-325 / R-325-1 (MUST-65): identity cannot be established -> reject.
fn check_not_inside_sources(
    items: &[PathBuf],
    output: &Path,
    output_exists: bool,
) -> Result<(), ArchiveError> {
    let inside = || {
        ArchiveError::new(
            Code::DestinationInsideSource,
            "the output file would be inside its own source",
        )
    };
    let output_norm = normalize_allowing_missing(output).ok_or_else(inside)?;
    for item in items {
        // Links are skipped by the creation itself (R-327-8).
        let meta = fs::symlink_metadata(item).map_err(|e| io_failure(&e))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let canonical = fs::canonicalize(item).map_err(|_| inside())?;
        if meta.is_dir() {
            if output_norm.starts_with(&canonical) {
                return Err(inside());
            }
        } else if output_norm == canonical
            || (output_exists && same_entity(item, output) != Some(false))
        {
            return Err(inside());
        }
    }
    Ok(())
}

/// R-327-9 boundary: `true` when the entry must be written with the ZIP64
/// large-file option (`len >= u32::MAX`, decided BEFORE writing).
pub fn needs_large_file(len: u64) -> bool {
    len >= u32::MAX as u64
}

/// Test seams.
pub struct CreateHooks<'a> {
    /// Polled at entry boundaries, while scanning and every 64 KiB (R-382).
    pub should_cancel: &'a dyn Fn() -> bool,
    /// Called before each entry is written (1-based index, total = number of
    /// entries that will be written, links excluded).
    pub on_progress: &'a dyn Fn(&ProgressEvent),
    /// MUST-66: called after the temp ZIP is complete and BEFORE the commit;
    /// receives the FINAL output path.
    pub before_commit: &'a dyn Fn(&Path),
}

fn never_cancel() -> bool {
    false
}
fn ignore_progress(_event: &ProgressEvent) {}
fn ignore_commit(_final_path: &Path) {}

impl CreateHooks<'static> {
    pub fn none() -> CreateHooks<'static> {
        CreateHooks {
            should_cancel: &never_cancel,
            on_progress: &ignore_progress,
            before_commit: &ignore_commit,
        }
    }
}

/// R-329.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateResult {
    pub output_path: PathBuf,
    /// Entries written (files + directory entries).
    pub entries: usize,
    /// Symlinks / junctions skipped (R-327-8).
    pub skipped_links: usize,
}

/// One entry to write.
struct Node {
    path: PathBuf,
    /// Archive name (`/` separated; directories without the trailing slash).
    name: String,
    is_dir: bool,
    len: u64,
    modified: Option<SystemTime>,
}

fn add_node(
    nodes: &mut Vec<Node>,
    path: &Path,
    name: String,
    meta: &fs::Metadata,
) -> Result<(), ArchiveError> {
    if name.len() > MAX_NAME_BYTES {
        return Err(ArchiveError::new(
            Code::InvalidPath,
            "an entry name is too long for the ZIP format",
        ));
    }
    nodes.push(Node {
        path: path.to_path_buf(),
        name,
        is_dir: meta.is_dir(),
        len: meta.len(),
        modified: meta.modified().ok(),
    });
    Ok(())
}

/// Walks one item WITHOUT following links (R-327-8), counting skipped links.
fn scan(
    path: &Path,
    name: String,
    nodes: &mut Vec<Node>,
    skipped_links: &mut usize,
    should_cancel: &dyn Fn() -> bool,
) -> Result<(), ArchiveError> {
    if should_cancel() {
        return Err(canceled());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| io_failure(&e))?;
    if meta.file_type().is_symlink() {
        *skipped_links += 1;
        return Ok(());
    }
    if meta.is_dir() {
        add_node(nodes, path, name.clone(), &meta)?;
        let mut children = Vec::new();
        for child in fs::read_dir(path).map_err(|e| io_failure(&e).with_entry(name.clone()))? {
            let child = child.map_err(|e| io_failure(&e).with_entry(name.clone()))?;
            children.push(child.path());
        }
        children.sort();
        for child in children {
            let child_name = item_name(&child).map_err(|e| e.with_entry(name.clone()))?;
            scan(
                &child,
                format!("{name}/{child_name}"),
                nodes,
                skipped_links,
                should_cancel,
            )?;
        }
    } else if meta.is_file() {
        add_node(nodes, path, name, &meta)?;
    }
    Ok(())
}

/// Creates the ZIP. Re-runs `check_create` (defence for non-UI callers); an
/// existing file with `overwrite == false` -> `conflict` (R-326-3). Writes to a
/// temp file (`temp::TempFile`, same directory) and commits with
/// `win_fs::move_no_replace` when `overwrite == false` (MUST-66) or a replacing
/// rename when `overwrite == true` (R-326-4). Failure / cancellation deletes
/// the temp file, leaves an existing output untouched and returns `Err` (a
/// cancel is `Err(canceled)`; no partial success, R-329). Cancellation raised
/// before any work creates nothing.
pub fn create_zip(
    items: &[PathBuf],
    output_path: &str,
    base_dir: &Path,
    overwrite: bool,
    hooks: &CreateHooks,
) -> Result<CreateResult, ArchiveError> {
    if (hooks.should_cancel)() {
        return Err(canceled());
    }
    let checked = check_create(items, output_path, base_dir)?;
    if checked.overwrite_required && !overwrite {
        return Err(ArchiveError::new(
            Code::Conflict,
            "the output file exists and overwriting was not confirmed",
        ));
    }
    let output = checked.output_path;

    let mut nodes = Vec::new();
    let mut skipped_links = 0usize;
    for item in items {
        scan(
            item,
            item_name(item)?,
            &mut nodes,
            &mut skipped_links,
            hooks.should_cancel,
        )?;
    }

    let parent = output
        .parent()
        .ok_or_else(|| ArchiveError::new(Code::DestinationNotFound, "no output folder"))?;
    let mut temp = TempFile::create_in(parent).map_err(|e| io_failure(&e))?;
    write_archive(temp.file(), &nodes, hooks)?;
    temp.file().flush().map_err(|e| io_failure(&e))?;
    temp.file().sync_all().map_err(|e| io_failure(&e))?;
    temp.close_handle();
    (hooks.before_commit)(&output);
    if overwrite {
        fs::rename(temp.path(), &output).map_err(|e| io_failure(&e))?;
    } else {
        move_no_replace(temp.path(), &output).map_err(|e| io_failure(&e))?;
    }
    let _ = temp.into_path(); // committed
    Ok(CreateResult {
        output_path: output,
        entries: nodes.len(),
        skipped_links,
    })
}

fn write_archive(file: &mut File, nodes: &[Node], hooks: &CreateHooks) -> Result<(), ArchiveError> {
    let mut writer = zip::ZipWriter::new(file);
    let total = nodes.len();
    for (position, node) in nodes.iter().enumerate() {
        if (hooks.should_cancel)() {
            return Err(canceled());
        }
        (hooks.on_progress)(&ProgressEvent {
            name: &node.name,
            index: position + 1,
            total,
        });
        let method = if node.is_dir {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let mut options = zip::write::SimpleFileOptions::default()
            .compression_method(method)
            .large_file(needs_large_file(node.len));
        if let Some(stamp) = node.modified.and_then(system_time_to_zip) {
            options = options.last_modified_time(stamp);
        }
        if node.is_dir {
            writer
                .add_directory(&node.name, options)
                .map_err(|e| zip_failure(e).with_entry(node.name.clone()))?;
        } else {
            writer
                .start_file(&node.name, options)
                .map_err(|e| zip_failure(e).with_entry(node.name.clone()))?;
            copy_source(&node.path, &mut writer, &node.name, hooks.should_cancel)?;
        }
    }
    writer.finish().map_err(zip_failure)?;
    Ok(())
}

/// Streams a source file into the current entry (never `read_to_end`).
fn copy_source<W: Write>(
    path: &Path,
    writer: &mut W,
    name: &str,
    should_cancel: &dyn Fn() -> bool,
) -> Result<(), ArchiveError> {
    let mut source = File::open(path).map_err(|e| io_failure(&e).with_entry(name))?;
    let mut buffer = vec![0u8; BLOCK];
    loop {
        if should_cancel() {
            return Err(canceled());
        }
        let read = match source.read(&mut buffer) {
            Ok(read) => read,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_failure(&e).with_entry(name)),
        };
        if read == 0 {
            return Ok(());
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|e| io_failure(&e).with_entry(name))?;
    }
}
