//! E2E file-operation scope (debug builds only). SPEC-0400-0001 R-10 .. R-10-6;
//! SPEC-0200-0001 §3.10.17.6 / §3.10.17.6-2 (MUST-123, SHOULD-178..180).
//!
//! While an E2E run is active the launcher points the app at ONE folder; every command that
//! changes files, reads a copy/ZIP source or launches another program refuses a path outside
//! that folder BEFORE touching the file system (a second line of defence behind the page-level
//! stubs of the E2E harness).
//!
//! The scope logic lives in `Scope` (injectable, no process-wide state); the free `check_*`
//! fns consult the process-wide scope set by `init_from_env` and are `Ok(())` when no scope is
//! set -- always, in release builds, which never read the environment (SHOULD-180: the variable
//! is only referenced inside the `#[cfg(debug_assertions)] mod imp` below).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// `OpSummary` failure code / `code=` prefix of command errors.
pub const VIOLATION_CODE: &str = "e2e_scope_violation";

/// Invalid `REFLEXFILES_E2E_FS_SCOPE` value (start-up error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeError(pub String);

impl std::fmt::Display for ScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid E2E scope value: {}", self.0)
    }
}

impl std::error::Error for ScopeError {}

/// A path outside the scope (or undecidable). Rejected before touching the file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeViolation(pub String);

impl ScopeViolation {
    /// `"code=e2e_scope_violation; <message>"` (R-10-3: how a refusal is expressed).
    pub fn to_command_error(&self) -> String {
        format!("code={VIOLATION_CODE}; {}", self.0)
    }
}

#[cfg(debug_assertions)]
mod imp {
    /// The only place that reads the environment (debug builds only).
    pub(super) fn read() -> Option<std::ffi::OsString> {
        std::env::var_os("REFLEXFILES_E2E_FS_SCOPE")
    }
}
#[cfg(not(debug_assertions))]
mod imp {
    /// Release builds never read the variable.
    pub(super) fn read() -> Option<std::ffi::OsString> {
        None
    }
}

/// Raw components of `path` (both separators), with a leading `\\?\` and the UNC `\\` removed.
fn raw_components(text: &str) -> Vec<&str> {
    let rest = text
        .strip_prefix(r"\\?\")
        .or_else(|| text.strip_prefix(r"\\"))
        .unwrap_or(text);
    rest.split(['\\', '/']).collect()
}

/// Lexical rules that hold for every component of a path (R-10-3 / SHOULD-178): no `.` / `..`,
/// not dots-and-spaces only, no trailing space or dot (Win32 trims them). Empty components
/// (a doubled separator, a trailing separator) are not names and are skipped.
fn lexically_safe(text: &str) -> bool {
    raw_components(text)
        .into_iter()
        .filter(|c| !c.is_empty())
        .all(|c| {
            c != "."
                && c != ".."
                && !c.chars().all(|ch| ch == '.' || ch == ' ')
                && !c.ends_with(' ')
                && !c.ends_with('.')
        })
}

fn lowered_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|c| match c {
            Component::Normal(name) => name.to_string_lossy().to_lowercase(),
            other => other.as_os_str().to_string_lossy().to_lowercase(),
        })
        .collect()
}

/// An active scope; `root` is the canonicalized folder.
#[derive(Debug, Clone)]
pub struct Scope {
    root: PathBuf,
}

impl Scope {
    pub fn new(canonical_root: PathBuf) -> Self {
        Scope {
            root: canonical_root,
        }
    }

    fn violation(&self, what: &str, path: &Path) -> ScopeViolation {
        ScopeViolation(format!(
            "{what}: {} (scope {})",
            path.display(),
            self.root.display()
        ))
    }

    /// Component-wise, case-insensitive prefix test of two canonical paths.
    fn contains(&self, canonical: &Path, allow_root: bool) -> bool {
        let root = lowered_components(&self.root);
        let cand = lowered_components(canonical);
        cand.len() >= root.len()
            && cand[..root.len()] == root[..]
            && (allow_root || cand.len() > root.len())
    }

    /// The path as text when it is absolute, non-empty and lexically safe.
    fn usable<'a>(&self, path: &'a Path, what: &str) -> Result<&'a str, ScopeViolation> {
        let text = path.to_str().filter(|t| !t.trim().is_empty());
        match text {
            Some(t) if path.is_absolute() && lexically_safe(t) => Ok(t),
            _ => Err(self.violation(&format!("{what}: not an absolute, plain path"), path)),
        }
    }

    /// Write destinations (copy/move destination, ZIP output, extract destination, create parent).
    /// The deepest EXISTING ancestor is canonicalized (links followed); the rest is appended.
    pub fn check_container(&self, path: &Path) -> Result<(), ScopeViolation> {
        self.usable(path, "container")?;
        let mut missing: Vec<OsString> = Vec::new();
        let mut cursor = path.to_path_buf();
        let canonical = loop {
            match std::fs::canonicalize(&cursor) {
                Ok(found) => break found,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let (Some(parent), Some(name)) = (cursor.parent(), cursor.file_name()) else {
                        return Err(self.violation("container cannot be resolved", path));
                    };
                    missing.push(name.to_os_string());
                    cursor = parent.to_path_buf();
                }
                Err(_) => return Err(self.violation("container cannot be resolved", path)),
            }
        };
        let full = missing
            .iter()
            .rev()
            .fold(canonical, |acc, name| acc.join(name));
        if self.contains(&full, true) {
            Ok(())
        } else {
            Err(self.violation("outside the E2E scope (container)", path))
        }
    }

    /// Parent canonicalized + the item's own name (the item itself is never followed).
    fn entry_resolved(&self, path: &Path, what: &str) -> Result<PathBuf, ScopeViolation> {
        self.usable(path, what)?;
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(self.violation(&format!("{what} has no parent"), path));
        };
        let parent = std::fs::canonicalize(parent)
            .map_err(|_| self.violation(&format!("{what}: parent cannot be resolved"), path))?;
        Ok(parent.join(name))
    }

    /// The item itself (delete, rename, move source): parent canonicalized + item name.
    pub fn check_entry(&self, path: &Path) -> Result<(), ScopeViolation> {
        let resolved = self.entry_resolved(path, "entry")?;
        if self.contains(&resolved, false) {
            Ok(())
        } else {
            Err(self.violation("outside the E2E scope (entry)", path))
        }
    }

    /// Read sources (copy/move items, ZIP items / base dir, ZIP to extract): entry rule AND the
    /// link-resolved canonical path inside the scope; canonicalize failure = reject (R-10-6).
    pub fn check_source(&self, path: &Path) -> Result<(), ScopeViolation> {
        let resolved = self.entry_resolved(path, "source")?;
        if !self.contains(&resolved, true) {
            return Err(self.violation("outside the E2E scope (source)", path));
        }
        let target = std::fs::canonicalize(path)
            .map_err(|_| self.violation("source cannot be resolved", path))?;
        if self.contains(&target, true) {
            Ok(())
        } else {
            Err(self.violation("source resolves outside the E2E scope (link)", path))
        }
    }

    /// `viewer_os_open_path`: ALWAYS rejected while the scope is active (R-10-5).
    pub fn check_external_launch(&self, path: &Path) -> Result<(), ScopeViolation> {
        Err(self.violation(
            "launching other programs is refused during an E2E run",
            path,
        ))
    }
}

/// R-10-1: `None` = variable not set (scope off). Errors: empty / blank / non-Unicode /
/// not absolute / contains `..` / does not exist (never created) / not a directory.
pub fn resolve_scope(raw: Option<&OsStr>) -> Result<Option<PathBuf>, ScopeError> {
    let Some(raw) = raw else { return Ok(None) };
    let text = raw
        .to_str()
        .ok_or_else(|| ScopeError("not valid Unicode".into()))?;
    if text.trim().is_empty() {
        return Err(ScopeError("empty".into()));
    }
    let path = Path::new(text);
    if !path.is_absolute() {
        return Err(ScopeError(format!("not an absolute path: {text}")));
    }
    if raw_components(text).contains(&"..") {
        return Err(ScopeError(format!("contains `..`: {text}")));
    }
    let canonical = std::fs::canonicalize(path).map_err(|e| ScopeError(format!("{text}: {e}")))?;
    if !canonical.is_dir() {
        return Err(ScopeError(format!("not a directory: {text}")));
    }
    Ok(Some(canonical))
}

static SCOPE: OnceLock<Scope> = OnceLock::new();

/// Reads the variable (debug builds), validates, stores the process-wide scope, prints
/// `E2E fs scope active: <canonical path>` to stderr.
pub fn init_from_env() -> Result<(), ScopeError> {
    let raw = imp::read();
    if let Some(root) = resolve_scope(raw.as_deref())? {
        eprintln!("E2E fs scope active: {}", root.display());
        let _ = SCOPE.set(Scope::new(root));
    }
    Ok(())
}

/// The scope the free `check_*` fns consult: the test override of this thread first
/// (`set_test_scope`, tests only), then the process-wide one set by `init_from_env`.
fn with_scope(f: impl Fn(&Scope) -> Result<(), ScopeViolation>) -> Result<(), ScopeViolation> {
    #[cfg(test)]
    {
        if let Some(result) = test_override::TEST_SCOPE.with(|cell| cell.borrow().as_ref().map(&f))
        {
            return result;
        }
    }
    SCOPE.get().map_or(Ok(()), f)
}

pub fn check_container(path: &Path) -> Result<(), ScopeViolation> {
    with_scope(|s| s.check_container(path))
}
pub fn check_entry(path: &Path) -> Result<(), ScopeViolation> {
    with_scope(|s| s.check_entry(path))
}
pub fn check_source(path: &Path) -> Result<(), ScopeViolation> {
    with_scope(|s| s.check_source(path))
}
pub fn check_external_launch(path: &Path) -> Result<(), ScopeViolation> {
    with_scope(|s| s.check_external_launch(path))
}

/// SPEC-0200-0001 §3.12.9 AC-88 / MINOR-11 (unit-test-writer): a `#[cfg(test)]` thread-local scope
/// override, OUTSIDE `mod imp`. `ScopeGuard::drop` always restores the previous value, so a test
/// never leaks a scope into another test (even with `--test-threads=1`, which reuses threads).
#[cfg(test)]
mod test_override {
    use super::Scope;
    use std::cell::RefCell;

    thread_local! {
        pub(super) static TEST_SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
    }
}

/// Restores the previous thread-local scope on drop.
#[cfg(test)]
pub struct ScopeGuard {
    previous: Option<Scope>,
}

#[cfg(test)]
impl Drop for ScopeGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        test_override::TEST_SCOPE.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// Activates `scope` for the free `check_*` fns on THIS thread until the guard is dropped.
#[cfg(test)]
pub fn set_test_scope(scope: Scope) -> ScopeGuard {
    let previous = test_override::TEST_SCOPE.with(|cell| cell.replace(Some(scope)));
    ScopeGuard { previous }
}
