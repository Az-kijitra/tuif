//! `archive/extract_plan.rs` (SPEC-0300-0014 §6 / §7: R-345, R-360~R-367-1,
//! R-371, R-372~R-372-2, R-375; SPEC-0200-0001 §3.4.4, §3.4.13-1 / -2).
//!
//! `build_plan` is the ONE validation implementation: both
//! `archive_detect_conflicts` and `archive_extract` call it (the latter again
//! right before writing; the TOCTOU window is closed at write time by
//! `extract`, R-376 / MUST-66). It never writes anything.
//!
//! Tests: `archive/extract_plan_tests.rs`.

use super::central_dir::{self, CdRecord};
use super::entry_name::resolve_entry_name;
use super::eocd;
use super::error::{classify_io_error, classify_zip_error, ArchiveError, ArchiveErrorCode as Code};
use super::path_rules::{check_path_lengths, fold_key, validate_entry_name};
use super::win_fs;
use super::zip_time::zip_to_system_time;
use crate::shared::identity::{normalize_allowing_missing, same_entity};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// L-2 (R-365): maximum sum of declared uncompressed sizes (16 GiB).
pub const MAX_TOTAL_UNCOMPRESSED: u64 = 16 << 30;
/// L-5 (R-365-4): maximum declared Deflate ratio.
pub const MAX_DEFLATE_RATIO: u64 = 1032;

/// Injection point for the free-space query (AC-372 / AC-372-1 / AC-380-1).
/// A plain function reference: NOT a Storage Provider abstraction.
pub struct ExtractEnv<'a> {
    /// Bytes available on the volume of the given (existing) path.
    pub free_space: &'a dyn Fn(&Path) -> io::Result<u64>,
}

impl ExtractEnv<'static> {
    /// The production environment (`win_fs::free_space`).
    pub fn real() -> ExtractEnv<'static> {
        ExtractEnv { free_space: &win_fs::free_space }
    }
}

/// One validated archive entry.
#[derive(Debug, Clone)]
pub struct PlanEntry {
    /// Index in the central directory.
    pub index: usize,
    /// Archive path used for progress / conflict keys / resolutions: the
    /// validated components joined with `/` (no trailing slash, `.` and empty
    /// components removed).
    pub entry_path: String,
    /// Validated components (R-361 / R-363), relative to the destination.
    pub components: Vec<String>,
    pub is_dir: bool,
    /// `destination_norm` joined with `components`.
    pub target: PathBuf,
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    /// Compression method: 0 = Stored, 8 = Deflate.
    pub method: u16,
    /// Entry modification time from the central directory, when valid (R-346-2).
    pub modified: Option<SystemTime>,
}

/// The result of a successful validation (nothing has been written).
#[derive(Debug, Clone)]
pub struct Plan {
    pub zip_path: PathBuf,
    /// Normalized destination (`normalize_allowing_missing`, R-343-2).
    pub destination: PathBuf,
    /// Number of entries in the central directory (R-381 total).
    pub total_entries: usize,
    /// Sum of declared uncompressed sizes (L-2 / L-3 basis).
    pub total_uncompressed: u64,
    pub entries: Vec<PlanEntry>,
    /// `entry_path`s of FILE entries whose final path already exists as a
    /// non-link, non-directory item (R-372). Directory-vs-directory is never a
    /// conflict (R-344 / R-374-3).
    pub conflicts: Vec<String>,
}

fn rejected(message: impl Into<String>) -> ArchiveError {
    ArchiveError::new(Code::ZipEntryRejected, message)
}

/// The crate's name for an index must be the name our central-directory scan
/// recorded for it; a difference is `zip_corrupt`.
pub(super) fn ensure_same_name(crate_name: &[u8], scanned_name: &[u8]) -> Result<(), ArchiveError> {
    if crate_name == scanned_name {
        return Ok(());
    }
    Err(ArchiveError::new(Code::ZipCorrupt, "the entry table is inconsistent")
        .with_entry(String::from_utf8_lossy(scanned_name).into_owned()))
}

fn io_failure(err: &io::Error) -> ArchiveError {
    ArchiveError::new(classify_io_error(err), err.to_string())
}

/// R-341-1 / R-343-1 (pure: no filesystem access): the first half of `resolve_destination`. Empty ->
/// `invalid_path`; absolute -> as given; drive-relative / root-relative -> `invalid_path`; relative -> joined
/// to `base_dir` (which must itself be absolute).
pub(crate) fn join_destination(destination: &Path, base_dir: &Path) -> Result<PathBuf, ArchiveError> {
    let has_prefix = matches!(destination.components().next(), Some(Component::Prefix(_)));
    if destination.as_os_str().is_empty() {
        Err(ArchiveError::new(Code::InvalidPath, "the destination is empty"))
    } else if destination.is_absolute() {
        Ok(destination.to_path_buf())
    } else if destination.has_root() || has_prefix {
        Err(ArchiveError::new(Code::InvalidPath, "a drive-relative or root-relative destination is ambiguous"))
    } else if base_dir.is_absolute() {
        Ok(base_dir.join(destination))
    } else {
        Err(ArchiveError::new(Code::InvalidPath, "the base directory is not an absolute path"))
    }
}

/// R-341-1 / R-343-1 / R-343-2: resolves `destination` against `base_dir`,
/// rejects an existing non-directory and normalizes (the missing tail gets the
/// R-363 checks). Shared by `build_plan` and the re-check in `extract`.
pub(crate) fn resolve_destination(destination: &Path, base_dir: &Path) -> Result<PathBuf, ArchiveError> {
    let joined = join_destination(destination, base_dir)?;
    if let Ok(meta) = fs::metadata(&joined) {
        if !meta.is_dir() {
            return Err(ArchiveError::new(Code::DestinationNotDir, "the destination is an existing file"));
        }
    }
    let normalized = normalize_allowing_missing(&joined)
        .ok_or_else(|| rejected("the destination path cannot be normalized safely"))?;
    // R-343-2 (2): the part that does not exist yet is checked like an entry name.
    let mut existing = normalized.clone();
    let mut tail: Vec<String> = Vec::new();
    while fs::symlink_metadata(&existing).is_err() {
        match existing.file_name() {
            Some(name) => tail.push(name.to_string_lossy().into_owned()),
            None => break,
        }
        if !existing.pop() {
            break;
        }
    }
    for component in tail {
        match validate_entry_name(&component) {
            Ok(parts) if parts.len() == 1 && parts[0] == component => {}
            _ => return Err(rejected(format!("the destination folder name `{component}` is not allowed"))),
        }
    }
    Ok(normalized)
}

/// Data taken from the crate and the own central-directory scan for one entry.
struct RawEntry {
    name: String,
    is_dir: bool,
    method: u16,
    uncompressed: u64,
    compressed: u64,
    modified: Option<SystemTime>,
}

/// Steps 2-3: EOCD pre-check, opening, per-entry metadata (central names only).
fn read_entries(zip_path: &Path) -> Result<Vec<RawEntry>, ArchiveError> {
    let info = eocd::inspect(zip_path)?;
    let file = File::open(zip_path).map_err(|e| io_failure(&e))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| ArchiveError::new(classify_zip_error(&e), format!("cannot read the ZIP archive: {e}")))?;
    let declared = usize::try_from(info.total_entries).unwrap_or(usize::MAX);

    let mut scan_file = File::open(zip_path).map_err(|e| io_failure(&e))?;
    let records = central_dir::scan(&mut scan_file, archive.central_directory_start(), declared)?;
    // Repeated directory records collapse into one (the crate does the same), so
    // the crate's indices line up with the de-duplicated list.
    let records = unique_records(records)?;
    if archive.len() != records.len() {
        return Err(ArchiveError::new(
            Code::ZipCorrupt,
            "the entry count of the central directory disagrees with the end record",
        ));
    }

    let mut entries = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        let entry_label = String::from_utf8_lossy(&record.name_raw).into_owned();
        // R-362 #3: bit 0 or method 99 (AES is invisible to the crate without bit 0).
        if record.flags & 0x0001 != 0 || record.method == 99 {
            return Err(ArchiveError::new(Code::ZipEncrypted, "the archive contains an encrypted entry")
                .with_entry(entry_label));
        }
        // R-362 #2: only Stored (0) and Deflate (8).
        if record.method != 0 && record.method != 8 {
            return Err(ArchiveError::new(
                Code::ZipUnsupported,
                format!("compression method {} is not supported", record.method),
            )
            .with_entry(entry_label));
        }
        let file = archive
            .by_index_raw(index)
            .map_err(|e| ArchiveError::new(classify_zip_error(&e), format!("cannot read entry: {e}")).with_entry(entry_label.clone()))?;
        ensure_same_name(file.name_raw(), &record.name_raw)?;
        // R-362 #6: symbolic-link entries.
        if file.is_symlink() {
            return Err(rejected("the archive contains a symbolic link entry").with_entry(entry_label));
        }
        let name = resolve_entry_name(file.name_raw(), record.flags & 0x0800 != 0).map_err(|e| e.with_entry(entry_label))?;
        let is_dir = name.ends_with('/') || name.ends_with('\\');
        entries.push(RawEntry {
            name,
            is_dir,
            method: record.method,
            uncompressed: if is_dir { 0 } else { file.size() },
            compressed: if is_dir { 0 } else { file.compressed_size() },
            modified: file.last_modified().and_then(|dt| zip_to_system_time(&dt)),
        });
    }
    Ok(entries)
}

/// R-364-1: the crate merges byte-identical names into one entry, so a
/// duplicate FILE name has to be found in the central directory itself. A
/// repeated directory record is dropped (allowed by R-364-1).
fn unique_records(records: Vec<CdRecord>) -> Result<Vec<CdRecord>, ArchiveError> {
    let mut seen: HashSet<Vec<u8>> = HashSet::with_capacity(records.len());
    let mut unique = Vec::with_capacity(records.len());
    for record in records {
        if seen.insert(record.name_raw.clone()) {
            unique.push(record);
            continue;
        }
        let name = String::from_utf8_lossy(&record.name_raw).into_owned();
        if !(name.ends_with('/') || name.ends_with('\\')) {
            return Err(rejected("the archive contains the same file name twice").with_entry(name));
        }
    }
    Ok(unique)
}

/// Validation order (SPEC-0200-0001 §3.4.4, MUST-70 / MUST-65):
/// 1. `zip_path` exists as a regular file (`not_found`); `destination` resolved
///    against `base_dir` (drive-relative / root-relative -> `invalid_path`),
///    an existing file -> `destination_not_dir`, `normalize_allowing_missing`
///    (`None` -> reject), the missing tail checked with R-363;
/// 2. `eocd::check_eocd` BEFORE `ZipArchive::new` (`zip_corrupt` /
///    `zip_unsupported` / L-1 `zip_too_large`); an open failure ->
///    `zip_corrupt`; entry count differing from the EOCD -> `zip_corrupt`;
/// 3. per entry, using the CENTRAL-DIRECTORY name only (R-361-1): encrypted
///    (bit 0 or method 99) -> `zip_encrypted`; method other than Stored/Deflate
///    -> `zip_unsupported`; symlink entry -> `zip_entry_rejected`; name
///    (`entry_name` + `path_rules`) -> `zip_entry_rejected` (R-363 #11 ->
///    `invalid_path`);
/// 4. duplicates / file-vs-dir (R-364, R-364-1, R-364-2) -> `zip_entry_rejected`;
/// 5. L-2 (`zip_too_large`, overflow-safe), L-5 / Stored mismatch (`zip_corrupt`);
/// 6. L-3 via `env.free_space(nearest existing ancestor)` -> `insufficient_space`;
///    an `Err` from `free_space` SKIPS the check (R-365-6);
/// 7. final paths: existing link at the final name -> `zip_entry_rejected`
///    (R-367); the final path is the ZIP itself -> `zip_entry_rejected`
///    (R-367-1: a non-existent final path is NOT the ZIP; only an existing one
///    is compared with `same_entity`, `None` -> reject); kind mismatch ->
///    `conflict` (R-372-1); ancestor link escaping the destination ->
///    `zip_entry_rejected`; existing file -> collected in `conflicts`.
pub fn build_plan(
    zip_path: &Path,
    destination: &Path,
    base_dir: &Path,
    env: &ExtractEnv,
) -> Result<Plan, ArchiveError> {
    // 1
    match fs::metadata(zip_path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Err(ArchiveError::new(Code::NotFound, "the ZIP path is not a file")),
        Err(e) => return Err(io_failure(&e)),
    }
    let destination = resolve_destination(destination, base_dir)?;
    let destination_text = destination.to_string_lossy().into_owned();

    // 2, 3
    let raw_entries = read_entries(zip_path)?;
    let total_entries = raw_entries.len();

    let mut entries = Vec::with_capacity(total_entries);
    for (index, raw) in raw_entries.into_iter().enumerate() {
        let components = validate_entry_name(&raw.name).map_err(|code| {
            ArchiveError::new(code, "the entry name is not allowed").with_entry(raw.name.clone())
        })?;
        let entry_path = components.join("/");
        check_path_lengths(&destination_text, &components, raw.is_dir)
            .map_err(|code| ArchiveError::new(code, "the extracted path would be too long").with_entry(entry_path.clone()))?;
        let mut target = destination.clone();
        for component in &components {
            target.push(component);
        }
        entries.push(PlanEntry {
            index,
            entry_path,
            components,
            is_dir: raw.is_dir,
            target,
            uncompressed_size: raw.uncompressed,
            compressed_size: raw.compressed,
            method: raw.method,
            modified: raw.modified,
        });
    }

    // 4
    check_duplicates(&entries)?;

    // 5
    let total_uncompressed = check_limits(&entries)?;

    // 6
    check_free_space(&destination, total_uncompressed, env)?;

    // 7
    let conflicts = check_targets(zip_path, &destination, &entries)?;

    Ok(Plan { zip_path: zip_path.to_path_buf(), destination, total_entries, total_uncompressed, entries, conflicts })
}

/// R-364 / R-364-1 / R-364-2 with the case-folded key of `path_rules::fold_key`.
fn check_duplicates(entries: &[PlanEntry]) -> Result<(), ArchiveError> {
    let mut dir_keys: HashSet<String> = HashSet::new();
    let mut file_keys: HashMap<String, &PlanEntry> = HashMap::new();
    for entry in entries {
        let parents = entry.components.len() - 1;
        for depth in 1..=parents {
            dir_keys.insert(fold_key(&entry.components[..depth]));
        }
        let key = fold_key(&entry.components);
        if entry.is_dir {
            dir_keys.insert(key);
        } else if file_keys.insert(key, entry).is_some() {
            return Err(rejected("the archive contains the same file name twice (letter case ignored)")
                .with_entry(entry.entry_path.clone()));
        }
    }
    for (key, entry) in &file_keys {
        if dir_keys.contains(key) {
            return Err(rejected("the archive contains a file and a folder with the same name")
                .with_entry(entry.entry_path.clone()));
        }
    }
    Ok(())
}

/// L-2 / L-5 (returns the declared total).
fn check_limits(entries: &[PlanEntry]) -> Result<u64, ArchiveError> {
    let mut total: u64 = 0;
    let mut overflowed = false;
    for entry in entries.iter().filter(|e| !e.is_dir) {
        let (uncompressed, compressed) = (entry.uncompressed_size, entry.compressed_size);
        match total.checked_add(uncompressed) {
            Some(sum) => total = sum,
            None => overflowed = true,
        }
        if entry.method == 0 {
            if compressed != uncompressed {
                return Err(ArchiveError::new(Code::ZipCorrupt, "a stored entry declares different compressed and uncompressed sizes")
                    .with_entry(entry.entry_path.clone()));
            }
        } else if (compressed == 0 && uncompressed > 0) || uncompressed > compressed.saturating_mul(MAX_DEFLATE_RATIO) {
            return Err(ArchiveError::new(Code::ZipCorrupt, "the declared compression ratio is implausible")
                .with_entry(entry.entry_path.clone()));
        }
    }
    if overflowed || total > MAX_TOTAL_UNCOMPRESSED {
        return Err(ArchiveError::new(
            Code::ZipTooLarge,
            "the declared size of the extracted data exceeds the 16 GiB limit",
        ));
    }
    Ok(total)
}

/// L-3 (R-365-1): free space of the nearest existing ancestor; a failing query skips the check.
fn check_free_space(destination: &Path, total: u64, env: &ExtractEnv) -> Result<(), ArchiveError> {
    let mut probe = destination.to_path_buf();
    while fs::symlink_metadata(&probe).is_err() {
        if !probe.pop() {
            return Ok(());
        }
    }
    match (env.free_space)(&probe) {
        Ok(free) if free < total => Err(ArchiveError::new(
            Code::InsufficientSpace,
            format!("not enough free space: {total} bytes needed, {free} available"),
        )),
        _ => Ok(()),
    }
}

/// Step 7: what already exists at the final paths.
fn check_targets(zip_path: &Path, destination: &Path, entries: &[PlanEntry]) -> Result<Vec<String>, ArchiveError> {
    let mut conflicts = Vec::new();
    // Nothing can exist below a destination that does not exist yet.
    if fs::symlink_metadata(destination).is_err() {
        return Ok(conflicts);
    }
    let mut verified_parents: HashSet<PathBuf> = HashSet::new();
    for entry in entries {
        if let Some(parent) = entry.target.parent() {
            if !verified_parents.contains(parent) {
                check_parent(parent, destination, &entry.entry_path)?;
                verified_parents.insert(parent.to_path_buf());
            }
        }
        let meta = match fs::symlink_metadata(&entry.target) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            // R-367: a link at the final name (whatever it points to, or not).
            let inside_dir = entry.is_dir
                && fs::canonicalize(&entry.target).map(|c| c.is_dir() && c.starts_with(destination)).unwrap_or(false);
            if !inside_dir {
                return Err(rejected("a link exists at the extraction path").with_entry(entry.entry_path.clone()));
            }
        } else if kind.is_dir() {
            if !entry.is_dir {
                return Err(ArchiveError::new(Code::Conflict, "a folder exists where a file would be extracted")
                    .with_entry(entry.entry_path.clone()));
            }
        } else if entry.is_dir {
            return Err(ArchiveError::new(Code::Conflict, "a file exists where a folder would be created")
                .with_entry(entry.entry_path.clone()));
        } else {
            // R-367-1 / MUST-65: only an EXISTING final path can be the ZIP itself.
            if same_entity(&entry.target, zip_path) != Some(false) {
                return Err(rejected("the entry would overwrite the archive itself").with_entry(entry.entry_path.clone()));
            }
            conflicts.push(entry.entry_path.clone());
        }
    }
    Ok(conflicts)
}

/// The nearest existing ancestor of `parent` must be a directory (or a link
/// to one) that resolves inside `destination` (R-366 advance check).
fn check_parent(parent: &Path, destination: &Path, entry_path: &str) -> Result<(), ArchiveError> {
    let mut probe = parent.to_path_buf();
    while fs::symlink_metadata(&probe).is_err() {
        if !probe.pop() {
            return Ok(());
        }
    }
    let canonical = fs::canonicalize(&probe)
        .map_err(|_| rejected("a link on the extraction path cannot be resolved").with_entry(entry_path))?;
    if !canonical.starts_with(destination) {
        return Err(rejected("a link on the extraction path leads outside the destination").with_entry(entry_path));
    }
    if !canonical.is_dir() {
        return Err(ArchiveError::new(Code::Conflict, "a file exists where a folder would be created").with_entry(entry_path));
    }
    Ok(())
}
