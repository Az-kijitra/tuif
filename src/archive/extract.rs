//! `archive/extract.rs` (SPEC-0300-0014 §5.3 / §5.4 / §6.3 / §7: R-343-2,
//! R-346~R-352, R-361-1, R-366~R-368, R-370, R-374, R-375, R-376, R-382,
//! R-384, R-385; SPEC-0200-0001 §3.4.4 execution, §3.4.13-2 MUST-66).
//!
//! Framework-agnostic: the Tauri layer is `archive/commands.rs`.
//! Tests: `archive/extract_tests.rs`.

use super::error::{classify_io_error, ArchiveError, ArchiveErrorCode as Code};
use super::extract_plan::{build_plan, resolve_destination, ExtractEnv, Plan, PlanEntry};
use super::progress::ProgressEvent;
use super::temp::TempFile;
use super::win_fs::move_no_replace;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Copy block size; also the cancellation polling granularity (R-382).
const BLOCK: usize = 64 * 1024;

/// Per-conflict decision supplied by the UI (R-374). A conflict with no entry
/// in the map is unresolved (R-375).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractOutcome {
    Completed,
    Failed,
    Canceled,
}

/// R-352.
#[derive(Debug, Clone)]
pub struct ExtractResult {
    /// Entries written (files + directories, NOT skipped ones).
    pub extracted: usize,
    /// Entries skipped by a `Skip` resolution.
    pub skipped: usize,
    /// Central-directory entry count (R-381).
    pub total: usize,
    pub destination: PathBuf,
    pub outcome: ExtractOutcome,
    /// The reason when `outcome` is not `Completed` (`canceled` for a cancel).
    pub error: Option<ArchiveError>,
}

/// Test seams (all default to the production behavior via `none()`).
pub struct ExtractHooks<'a> {
    /// Polled at every entry boundary and every 64 KiB while copying (R-382).
    pub should_cancel: &'a dyn Fn() -> bool,
    /// Called at the start of every entry that is processed (1-based index).
    pub on_progress: &'a dyn Fn(&ProgressEvent),
    /// MUST-66: called after an entry's temp file is fully written and BEFORE
    /// the commit; receives the FINAL path. Tests create a same-named file here.
    pub before_commit: &'a dyn Fn(&Path),
    /// R-346-2: sets a modification time; a failure must not fail the
    /// extraction. Called for files after their commit and for newly created
    /// directories after all their children are done.
    pub set_modified: &'a dyn Fn(&Path, SystemTime) -> io::Result<()>,
}

fn never_cancel() -> bool {
    false
}
fn ignore_progress(_event: &ProgressEvent) {}
fn ignore_commit(_final_path: &Path) {}

/// Opens a file OR directory for a timestamp update (Windows needs
/// `FILE_FLAG_BACKUP_SEMANTICS` to open a directory).
fn open_for_times(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new().write(true).custom_flags(0x0200_0000).open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

fn default_set_modified(path: &Path, time: SystemTime) -> io::Result<()> {
    open_for_times(path)?.set_modified(time)
}

impl ExtractHooks<'static> {
    pub fn none() -> ExtractHooks<'static> {
        ExtractHooks {
            should_cancel: &never_cancel,
            on_progress: &ignore_progress,
            before_commit: &ignore_commit,
            set_modified: &default_set_modified,
        }
    }
}

fn canceled() -> ArchiveError {
    ArchiveError::new(Code::Canceled, "the operation was canceled")
}

fn rejected(message: &str, entry: &PlanEntry) -> ArchiveError {
    ArchiveError::new(Code::ZipEntryRejected, message).with_entry(entry.entry_path.clone())
}

fn io_failure(err: &io::Error, entry: &PlanEntry) -> ArchiveError {
    ArchiveError::new(classify_io_error(err), err.to_string()).with_entry(entry.entry_path.clone())
}

/// Full extraction. `Err` = rejected before anything was written (every
/// `build_plan` error, and `conflict` for an unresolved conflict, R-375); when
/// cancellation was requested before any work the result is `Err(canceled)` and
/// nothing (not even the destination directory) is written (R-387-2). `Ok` =
/// writing started: the first error stops the run, already extracted entries
/// stay (R-350).
///
/// Per file entry: containment check of the parent (canonicalize, per entry,
/// never cached, R-366) -> temp file in the target directory
/// (`temp::TempFile`, fixed-length name) -> `copy_entry` -> `before_commit` ->
/// commit: `win_fs::move_no_replace` for a non-`Overwrite` entry (an existing
/// file appearing meanwhile -> `conflict`, R-376 / MUST-66) or a replacing
/// rename for an `Overwrite` entry -> `set_modified`.
pub fn extract(
    zip_path: &Path,
    destination: &Path,
    base_dir: &Path,
    env: &ExtractEnv,
    resolutions: &HashMap<String, Resolution>,
    hooks: &ExtractHooks,
) -> Result<ExtractResult, ArchiveError> {
    if (hooks.should_cancel)() {
        return Err(canceled());
    }
    // The same validation as `archive_detect_conflicts` (R-371), again right
    // before writing.
    let plan = build_plan(zip_path, destination, base_dir, env)?;
    for conflict in &plan.conflicts {
        if !resolutions.contains_key(conflict) {
            return Err(ArchiveError::new(Code::Conflict, "an existing file needs an overwrite/skip decision (R-375)")
                .with_entry(conflict.clone()));
        }
    }
    let file = File::open(zip_path).map_err(|e| ArchiveError::new(classify_io_error(&e), e.to_string()))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| ArchiveError::new(super::error::classify_zip_error(&e), e.to_string()))?;

    // R-387-2: a cancel that arrived while the plan was built writes nothing.
    if (hooks.should_cancel)() {
        return Err(canceled());
    }
    // From here on writing starts.
    fs::create_dir_all(&plan.destination).map_err(|e| ArchiveError::new(classify_io_error(&e), e.to_string()))?;
    // R-343-2 (3): the created destination must normalize to what was validated.
    match resolve_destination(&plan.destination, base_dir) {
        Ok(again) if again == plan.destination => {}
        _ => return Err(ArchiveError::new(Code::ZipEntryRejected, "the destination changed while it was being created")),
    }

    let conflict_set = plan.conflicts.iter().map(String::as_str).collect();
    let mut run = Run { plan: &plan, conflict_set, extracted: 0, skipped: 0, new_dirs: Vec::new() };
    let mut outcome = ExtractOutcome::Completed;
    let mut error = None;
    for (position, entry) in plan.entries.iter().enumerate() {
        if (hooks.should_cancel)() {
            outcome = ExtractOutcome::Canceled;
            error = Some(canceled());
            break;
        }
        (hooks.on_progress)(&ProgressEvent { name: &entry.entry_path, index: position + 1, total: plan.total_entries });
        match run.process(&mut archive, entry, resolutions.get(&entry.entry_path).copied(), hooks) {
            Ok(()) => {}
            Err(e) => {
                outcome = if e.code == Code::Canceled { ExtractOutcome::Canceled } else { ExtractOutcome::Failed };
                error = Some(e);
                break;
            }
        }
    }
    // R-346-2: new directories are stamped after all their children (deepest
    // first); a failure never fails the extraction.
    for (path, time) in run.new_dirs.iter().rev() {
        let _ = (hooks.set_modified)(path, *time);
    }
    Ok(ExtractResult {
        extracted: run.extracted,
        skipped: run.skipped,
        total: plan.total_entries,
        destination: plan.destination.clone(),
        outcome,
        error,
    })
}

/// Mutable state of one run.
struct Run<'p> {
    plan: &'p Plan,
    conflict_set: HashSet<&'p str>,
    extracted: usize,
    skipped: usize,
    /// Directories created by this run that have a modification time to restore.
    new_dirs: Vec<(PathBuf, SystemTime)>,
}

impl Run<'_> {
    fn process(
        &mut self,
        archive: &mut zip::ZipArchive<BufReader<File>>,
        entry: &PlanEntry,
        resolution: Option<Resolution>,
        hooks: &ExtractHooks,
    ) -> Result<(), ArchiveError> {
        // A decision only counts for an entry that was planned as a conflict.
        let resolution = if self.conflict_set.contains(entry.entry_path.as_str()) { resolution } else { None };
        if !entry.is_dir && resolution == Some(Resolution::Skip) {
            self.skipped += 1;
            return Ok(());
        }
        self.prepare_parent(entry)?;
        if entry.is_dir {
            self.make_dir(entry)?;
        } else {
            self.write_file(archive, entry, resolution == Some(Resolution::Overwrite), hooks)?;
        }
        self.extracted += 1;
        Ok(())
    }

    /// R-366: the parent must resolve inside the destination BEFORE anything
    /// is created, and again after the missing directories were created.
    /// Nothing is cached: every entry is verified from scratch.
    fn prepare_parent(&self, entry: &PlanEntry) -> Result<(), ArchiveError> {
        let parent = match entry.target.parent() {
            Some(parent) => parent,
            None => return Err(rejected("the entry has no parent folder", entry)),
        };
        let mut probe = parent.to_path_buf();
        while fs::symlink_metadata(&probe).is_err() {
            if !probe.pop() {
                return Err(rejected("no existing ancestor for the extraction path", entry));
            }
        }
        self.check_inside(&probe, entry)?;
        fs::create_dir_all(parent).map_err(|e| io_failure(&e, entry))?;
        self.check_inside(parent, entry)
    }

    /// `path` must canonicalize to a directory inside the destination.
    fn check_inside(&self, path: &Path, entry: &PlanEntry) -> Result<(), ArchiveError> {
        let canonical = fs::canonicalize(path).map_err(|_| rejected("a link on the extraction path cannot be resolved", entry))?;
        if !canonical.starts_with(&self.plan.destination) {
            return Err(rejected("a link on the extraction path leads outside the destination", entry));
        }
        if !canonical.is_dir() {
            return Err(ArchiveError::new(Code::Conflict, "a file exists where a folder is needed").with_entry(entry.entry_path.clone()));
        }
        Ok(())
    }

    fn make_dir(&mut self, entry: &PlanEntry) -> Result<(), ArchiveError> {
        let mut created = false;
        match fs::symlink_metadata(&entry.target) {
            Ok(meta) if meta.file_type().is_symlink() => self.check_inside(&entry.target, entry)?,
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(ArchiveError::new(Code::Conflict, "a file exists where a folder would be created")
                    .with_entry(entry.entry_path.clone()))
            }
            Err(_) => {
                match fs::create_dir(&entry.target) {
                    Ok(()) => created = true,
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(io_failure(&e, entry)),
                }
                self.check_inside(&entry.target, entry)?;
            }
        }
        if let (true, Some(time)) = (created, entry.modified) {
            self.new_dirs.push((entry.target.clone(), time));
        }
        Ok(())
    }

    fn write_file(
        &mut self,
        archive: &mut zip::ZipArchive<BufReader<File>>,
        entry: &PlanEntry,
        overwrite: bool,
        hooks: &ExtractHooks,
    ) -> Result<(), ArchiveError> {
        // R-367 / R-376: re-inspect the final name right before writing.
        match fs::symlink_metadata(&entry.target) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(rejected("a link exists at the extraction path", entry)),
            Ok(meta) if meta.is_dir() => {
                return Err(ArchiveError::new(Code::Conflict, "a folder exists where a file would be extracted")
                    .with_entry(entry.entry_path.clone()))
            }
            Ok(_) if !overwrite => {
                return Err(ArchiveError::new(Code::Conflict, "a file appeared at the extraction path")
                    .with_entry(entry.entry_path.clone()))
            }
            _ => {}
        }
        let parent = entry.target.parent().unwrap_or(&self.plan.destination);
        let mut temp = TempFile::create_in(parent).map_err(|e| io_failure(&e, entry))?;
        if entry.uncompressed_size > 0 {
            // R-374-1 special case: a declared size of 0 is never read (a
            // Deflate stream of 0 compressed bytes is an `UnexpectedEof`).
            let mut zip_file = archive
                .by_index(entry.index)
                .map_err(|e| ArchiveError::new(super::error::classify_zip_error(&e), e.to_string()).with_entry(entry.entry_path.clone()))?;
            copy_entry(&mut zip_file, temp.file(), entry.uncompressed_size, hooks.should_cancel)
                .map_err(|e| e.with_entry(entry.entry_path.clone()))?;
        }
        temp.file().flush().map_err(|e| io_failure(&e, entry))?;
        temp.close_handle();
        (hooks.before_commit)(&entry.target);
        if overwrite {
            fs::rename(temp.path(), &entry.target).map_err(|e| io_failure(&e, entry))?;
        } else {
            move_no_replace(temp.path(), &entry.target).map_err(|e| io_failure(&e, entry))?;
        }
        let _ = temp.into_path(); // committed: keep the (renamed-away) name from being deleted
        if let Some(time) = entry.modified {
            let _ = (hooks.set_modified)(&entry.target, time);
        }
        Ok(())
    }
}

/// A reader-side error that means "the archive data is broken" (flate2 reports
/// bad streams as `InvalidInput`, the crate's CRC check as `InvalidData`).
fn classify_read_error(err: &io::Error) -> Code {
    match err.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof => Code::ZipCorrupt,
        _ => classify_io_error(err),
    }
}

/// Streams one entry. Reads at most `declared_size + 1` bytes; more than
/// `declared_size` -> `zip_corrupt` (L-4, R-349); fewer -> `zip_corrupt`; a
/// reader error of kind `InvalidData` / `UnexpectedEof` (CRC mismatch, broken
/// stream) -> `zip_corrupt`; a writer error is classified by
/// `error::classify_io_error` (disk full -> `io_error`, R-395-2); `should_cancel`
/// is polled every 64 KiB -> `canceled`. Returns the bytes written.
pub fn copy_entry<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    declared_size: u64,
    should_cancel: &dyn Fn() -> bool,
) -> Result<u64, ArchiveError> {
    let mut buffer = vec![0u8; BLOCK];
    let mut total: u64 = 0;
    loop {
        if should_cancel() {
            return Err(canceled());
        }
        let want = (declared_size - total).saturating_add(1).min(BLOCK as u64) as usize;
        let read = match reader.read(&mut buffer[..want]) {
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(ArchiveError::new(classify_read_error(&e), format!("cannot read the entry data: {e}"))),
        };
        if read == 0 {
            break;
        }
        let allowed = (declared_size - total).min(read as u64) as usize;
        writer
            .write_all(&buffer[..allowed])
            .map_err(|e| ArchiveError::new(classify_io_error(&e), format!("cannot write the file: {e}")))?;
        total += allowed as u64;
        if allowed < read {
            return Err(ArchiveError::new(Code::ZipCorrupt, "the entry holds more data than its declared size"));
        }
    }
    if total < declared_size {
        return Err(ArchiveError::new(Code::ZipCorrupt, "the entry holds less data than its declared size"));
    }
    Ok(total)
}
