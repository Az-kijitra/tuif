//! TDD (unit-test-writer, 2026-10-07): the inner layer of the `fs_ops` commands refuses every
//! non-absolute path argument BEFORE touching the file system.
//! Specs: SPEC-0300-0011 第9版 R-89 / R-89-1 #3..#9 / R-89-2 / R-89-4 / R-89-6-1, T-04 / T-08,
//! AC-83 / AC-84 / AC-85 / AC-88 (S-3: the inner layer is called, then the file-system state is checked);
//! SPEC-0200-0001 §3.12.3 / §3.12.9.
//!
//! The process working directory is NEVER changed (the lib tests run in parallel and other tests
//! use relative paths). Relative paths are made of a random name `zz-abs-path-<unique>`; after each
//! refusal the name must not exist in the working directory, and the absolute folders U / V must be
//! unchanged. The direct "a.txt placed in T and pointed at relatively" proof is the E2E AC-256/257
//! (SPEC-0200-0001 §3.12.9, M-3).

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use super::guarded::{
    copy_guarded, create_guarded, delete_permanently_guarded, delete_trash_guarded, detect_conflicts_guarded,
    move_guarded, rename_guarded,
};
use super::test_support::{names, tempdir, write};
use super::transfer::{ConflictResolution, OpSummary};
use crate::shared::e2e_scope::{set_test_scope, Scope, VIOLATION_CODE};

const INVALID: &str = "invalid_path";

// ------------------------------------------------------------------------------------ helpers

fn rand_name() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("zz-abs-path-{nanos}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// The drive letter of the working directory (`C` ...): `X:name` / `\name` are relative to IT.
fn cwd_drive() -> char {
    let cwd = std::env::current_dir().expect("cwd").to_string_lossy().trim_start_matches(r"\\?\").to_string();
    cwd.chars().next().filter(|c| c.is_ascii_alphabetic()).unwrap_or('C')
}

/// Removes the random relative names from the working directory when the test ends (also on failure),
/// so a refusal that did not happen cannot leave debris behind.
struct Debris(Vec<String>);
impl Drop for Debris {
    fn drop(&mut self) {
        for name in &self.0 {
            let _ = fs::remove_dir_all(name).or_else(|_| fs::remove_file(name));
        }
    }
}

/// (i) relative, (ii) drive-relative (`X:name`, `X:`), (iii) root-relative, (iv) empty, plus `/` and `.\` forms.
fn bad_forms(rand: &str) -> Vec<String> {
    let d = cwd_drive();
    vec![
        rand.to_string(),
        format!(r".\{rand}"),
        format!("{d}:{rand}"),
        format!("{d}:"),
        format!(r"\{rand}"),
        format!(r"\{rand}\a.txt"),
        String::new(),
        format!("{d}:/{rand}"),
        format!("{rand}/x"),
    ]
}

/// Every file and folder below `dir` with its size, sorted: a cheap "nothing changed" fingerprint.
fn snapshot(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().display().to_string();
            if e.file_type().unwrap().is_dir() {
                out.push(format!("{rel}\\"));
                walk(base, &p, out);
            } else {
                out.push(format!("{rel}:{}", fs::metadata(&p).unwrap().len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// U (a source side) with `a.txt` and a folder `d`, V (an empty destination), both absolute.
struct World {
    u: tempfile::TempDir,
    v: tempfile::TempDir,
    rand: String,
    _debris: Debris,
}

fn world() -> World {
    let u = tempdir();
    let v = tempdir();
    write(u.path(), "a.txt", "A");
    write(u.path(), "d/inner.txt", "I");
    let rand = rand_name();
    let debris = Debris(vec![rand.clone()]);
    World { u, v, rand, _debris: debris }
}

impl World {
    fn a(&self) -> String {
        s(&self.u.path().join("a.txt"))
    }
    fn d(&self) -> String {
        s(&self.u.path().join("d"))
    }
    fn dest(&self) -> String {
        s(self.v.path())
    }
    fn before(&self) -> (Vec<String>, Vec<String>) {
        (snapshot(self.u.path()), snapshot(self.v.path()))
    }
    fn assert_unchanged(&self, before: &(Vec<String>, Vec<String>), what: &str) {
        assert_eq!(&self.before(), before, "{what}: U / V changed");
        assert!(!Path::new(&self.rand).exists(), "{what}: the random relative name appeared in the working directory");
    }
}

/// R-89-1 / T-04: every item failed with `invalid_path`, nothing counted as done.
fn assert_refused_all(summary: &OpSummary, items: &[String], what: &str) {
    let n = items.len() as u32;
    assert_eq!((summary.ok, summary.failed, summary.total, summary.skipped), (0, n, n, 0), "{what}: {summary:?}");
    assert_eq!(summary.failures.len(), items.len(), "{what}");
    for (failure, item) in summary.failures.iter().zip(items) {
        assert_eq!(failure.code, INVALID, "{what}: {failure:?}");
        assert_eq!(failure.path, PathBuf::from(item), "{what}: the failure names the item as given");
    }
}

struct Probe {
    events: Cell<u32>,
    resolved: Cell<u32>,
}
impl Probe {
    fn new() -> Self {
        Probe { events: Cell::new(0), resolved: Cell::new(0) }
    }
}

fn copy(items: &[String], dest: &str, probe: &Probe) -> OpSummary {
    copy_guarded(
        items,
        dest,
        |_| {
            probe.resolved.set(probe.resolved.get() + 1);
            ConflictResolution::Reject
        },
        || false,
        |_| probe.events.set(probe.events.get() + 1),
    )
}

fn mv(items: &[String], dest: &str, probe: &Probe) -> OpSummary {
    move_guarded(
        items,
        dest,
        |_| {
            probe.resolved.set(probe.resolved.get() + 1);
            ConflictResolution::Reject
        },
        || false,
        |_| probe.events.set(probe.events.get() + 1),
    )
}

// ------------------------------------------------------------------------------------ AC-83 copy / move

// AC-83 (i)..(iv), items side: any bad item refuses the whole batch (T-04) and nothing is copied.
#[test]
fn ac83_copy_with_a_bad_items_entry_refuses_everything() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        let items = vec![w.a(), bad.clone()];
        let probe = Probe::new();
        let summary = copy(&items, &w.dest(), &probe);
        assert_refused_all(&summary, &items, &format!("copy items {bad:?}"));
        assert_eq!((probe.events.get(), probe.resolved.get()), (0, 0), "no progress event, no resolver call: {bad:?}");
        w.assert_unchanged(&before, &format!("copy items {bad:?}"));
    }
}

#[test]
fn ac83_copy_with_a_bad_destination_refuses_every_item() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        let items = vec![w.a(), w.d()];
        let probe = Probe::new();
        let summary = copy(&items, &bad, &probe);
        assert_refused_all(&summary, &items, &format!("copy destination {bad:?}"));
        assert_eq!((probe.events.get(), probe.resolved.get()), (0, 0), "{bad:?}");
        w.assert_unchanged(&before, &format!("copy destination {bad:?}"));
    }
}

#[test]
fn ac83_move_with_a_bad_items_entry_or_destination_refuses_everything() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        let items = vec![w.a(), bad.clone()];
        let probe = Probe::new();
        assert_refused_all(&mv(&items, &w.dest(), &probe), &items, &format!("move items {bad:?}"));
        let items = vec![w.a(), w.d()];
        assert_refused_all(&mv(&items, &bad, &probe), &items, &format!("move destination {bad:?}"));
        assert_eq!((probe.events.get(), probe.resolved.get()), (0, 0), "{bad:?}");
        w.assert_unchanged(&before, &format!("move {bad:?}"));
    }
    assert!(Path::new(&w.a()).exists() && Path::new(&w.d()).exists(), "the sources are still where they were");
}

// AC-84 / T-04: ONE bad item among valid ones -> all fail, the valid one is NOT copied / moved.
#[test]
fn ac84_a_single_relative_item_fails_the_whole_batch_and_the_valid_item_is_untouched() {
    let w = world();
    let before = w.before();
    let items = vec![w.a(), w.rand.clone()];
    let probe = Probe::new();
    let summary = copy(&items, &w.dest(), &probe);
    assert_refused_all(&summary, &items, "copy");
    let summary = mv(&items, &w.dest(), &probe);
    assert_refused_all(&summary, &items, "move");
    assert!(names(w.v.path()).is_empty(), "nothing arrived in the destination");
    w.assert_unchanged(&before, "AC-84");
}

// R-89-2: the message names the refused path.
#[test]
fn the_failure_message_names_the_first_refused_path() {
    let w = world();
    let items = vec![w.a(), w.rand.clone(), "C:".to_string()];
    let summary = copy(&items, &w.dest(), &Probe::new());
    for failure in &summary.failures {
        assert!(failure.message.contains(&w.rand), "message should contain the first relative path: {failure:?}");
    }
}

// An empty batch is "total = 0" as before (the UI never sends one), whatever the destination.
#[test]
fn an_empty_items_list_gives_an_all_zero_summary() {
    let w = world();
    let probe = Probe::new();
    for dest in [w.dest(), w.rand.clone()] {
        for summary in [copy(&[], &dest, &probe), mv(&[], &dest, &probe)] {
            assert_eq!((summary.ok, summary.failed, summary.total, summary.skipped), (0, 0, 0, 0));
            assert!(summary.failures.is_empty());
        }
    }
}

// AC-85 (regression): absolute arguments still work.
#[test]
fn ac85_copy_and_move_with_absolute_paths_still_succeed() {
    let w = world();
    let probe = Probe::new();
    let summary = copy(&[w.a()], &w.dest(), &probe);
    assert_eq!((summary.ok, summary.failed, summary.total), (1, 0, 1), "{summary:?}");
    assert_eq!(fs::read_to_string(w.v.path().join("a.txt")).unwrap(), "A");
    assert!(probe.events.get() > 0, "progress events are emitted for a real batch");
    let w2 = world();
    let summary = mv(&[w2.a()], &w2.dest(), &Probe::new());
    assert_eq!((summary.ok, summary.failed, summary.total), (1, 0, 1), "{summary:?}");
    assert!(!Path::new(&w2.a()).exists());
    assert!(w2.v.path().join("a.txt").exists());
}

// ------------------------------------------------------------------------------------ AC-83 detect_conflicts

#[test]
fn ac83_detect_conflicts_with_a_bad_items_entry_or_destination_is_invalid_path() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        assert_eq!(detect_conflicts_guarded(&[w.a(), bad.clone()], &w.dest()).unwrap_err(), INVALID, "items {bad:?}");
        assert_eq!(detect_conflicts_guarded(&[w.a()], &bad).unwrap_err(), INVALID, "destination {bad:?}");
    }
    w.assert_unchanged(&before, "detect_conflicts");
}

#[test]
fn ac85_detect_conflicts_with_absolute_paths_still_works() {
    let w = world();
    write(w.v.path(), "a.txt", "already there");
    let conflicts = detect_conflicts_guarded(&[w.a()], &w.dest()).expect("absolute paths");
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].name, "a.txt");
}

// ------------------------------------------------------------------------------------ AC-83 delete

#[test]
fn ac83_delete_trash_with_a_bad_path_refuses_everything_and_deletes_nothing() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        let paths = vec![w.a(), bad.clone()];
        assert_refused_all(&delete_trash_guarded(&paths), &paths, &format!("trash {bad:?}"));
    }
    w.assert_unchanged(&before, "delete_trash");
    assert!(Path::new(&w.a()).exists() && Path::new(&w.d()).exists());
}

#[test]
fn ac83_delete_permanently_with_a_bad_path_refuses_everything_and_deletes_nothing() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        let paths = vec![w.d(), bad.clone()];
        assert_refused_all(&delete_permanently_guarded(&paths), &paths, &format!("permanent {bad:?}"));
    }
    w.assert_unchanged(&before, "delete_permanently");
    assert!(Path::new(&w.a()).exists() && Path::new(&w.d()).exists());
}

#[test]
fn ac84_delete_with_one_relative_path_among_absolute_ones_deletes_none() {
    let w = world();
    let before = w.before();
    let paths = vec![w.a(), w.rand.clone()];
    assert_refused_all(&delete_trash_guarded(&paths), &paths, "trash");
    assert_refused_all(&delete_permanently_guarded(&paths), &paths, "permanent");
    w.assert_unchanged(&before, "AC-84 delete");
}

#[test]
fn ac85_delete_permanently_with_absolute_paths_still_deletes() {
    let w = world();
    let summary = delete_permanently_guarded(&[w.a()]);
    assert_eq!((summary.ok, summary.failed, summary.total), (1, 0, 1), "{summary:?}");
    assert!(!Path::new(&w.a()).exists());
}

// ------------------------------------------------------------------------------------ AC-83 rename / create

#[test]
fn ac83_rename_with_a_bad_path_is_invalid_path_and_changes_nothing() {
    let w = world();
    let before = w.before();
    for bad in bad_forms(&w.rand) {
        assert_eq!(rename_guarded(&bad, "y").unwrap_err(), INVALID, "{bad:?}");
    }
    w.assert_unchanged(&before, "rename");
}

#[test]
fn ac83_create_with_a_bad_parent_is_invalid_path_and_creates_nothing() {
    let w = world();
    let before = w.before();
    let name = rand_name();
    let _debris = Debris(vec![name.clone()]);
    for bad in bad_forms(&w.rand) {
        assert_eq!(create_guarded(&bad, &name, "file").unwrap_err(), INVALID, "{bad:?} file");
        assert_eq!(create_guarded(&bad, &name, "folder").unwrap_err(), INVALID, "{bad:?} folder");
    }
    w.assert_unchanged(&before, "create");
    assert!(!Path::new(&name).exists(), "no entry named {name} appeared in the working directory");
}

// AC-85: absolute arguments work as before; a bad NAME is still `invalid_name` (names are out of scope, R-89-5).
#[test]
fn ac85_create_and_rename_with_absolute_paths_still_succeed() {
    let w = world();
    create_guarded(&s(w.u.path()), "x", "file").expect("create x");
    assert!(w.u.path().join("x").is_file());
    let renamed = rename_guarded(&s(&w.u.path().join("x")), "y").expect("rename x -> y");
    assert_eq!(PathBuf::from(&renamed), w.u.path().join("y"));
    assert!(w.u.path().join("y").exists() && !w.u.path().join("x").exists());
}

#[test]
fn ac85_a_parent_dir_name_is_still_invalid_name_not_invalid_path() {
    let w = world();
    let before = w.before();
    assert_eq!(create_guarded(&s(w.u.path()), r"..\x", "file").unwrap_err(), "invalid_name");
    assert_eq!(rename_guarded(&w.a(), r"..\y").unwrap_err(), "invalid_name");
    w.assert_unchanged(&before, "invalid names");
}

// ------------------------------------------------------------------------------------ AC-88 (before the E2E scope)

fn with_scope_on(w: &World) -> crate::shared::e2e_scope::ScopeGuard {
    // the scope is V; U (where a.txt lives) and the working directory are OUTSIDE it
    set_test_scope(Scope::new(fs::canonicalize(w.v.path()).unwrap()))
}

// AC-88 / R-89-4 / T-08: a relative path is `invalid_path`, never `e2e_scope_violation`, while a scope is active.
#[test]
fn ac88_with_a_scope_active_relative_paths_are_still_invalid_path() {
    let w = world();
    let _scope = with_scope_on(&w);
    let rel = w.rand.clone();
    let probe = Probe::new();

    for summary_items in [vec![rel.clone()], vec![w.a(), rel.clone()]] {
        assert_refused_all(&copy(&summary_items, &w.dest(), &probe), &summary_items, "copy items");
        assert_refused_all(&mv(&summary_items, &w.dest(), &probe), &summary_items, "move items");
        assert_refused_all(&delete_trash_guarded(&summary_items), &summary_items, "trash");
        assert_refused_all(&delete_permanently_guarded(&summary_items), &summary_items, "permanent");
    }
    let items = vec![w.a()];
    assert_refused_all(&copy(&items, &rel, &probe), &items, "copy destination");
    assert_refused_all(&mv(&items, &rel, &probe), &items, "move destination");
    assert_eq!(detect_conflicts_guarded(&items, &rel).unwrap_err(), INVALID);
    assert_eq!(rename_guarded(&rel, "y").unwrap_err(), INVALID);
    assert_eq!(create_guarded(&rel, "x", "file").unwrap_err(), INVALID);
    assert!(!Path::new(&rel).exists());
}

// Control: the same scope really is active -- an ABSOLUTE path outside it IS an `e2e_scope_violation`.
#[test]
fn ac88_control_an_absolute_path_outside_the_scope_is_an_e2e_scope_violation() {
    let w = world();
    let _scope = with_scope_on(&w);
    let items = vec![w.a()]; // U is outside the scope V (as a read source)
    let summary = copy(&items, &w.dest(), &Probe::new());
    assert_eq!((summary.ok, summary.failed, summary.total), (0, 1, 1), "{summary:?}");
    assert_eq!(summary.failures[0].code, VIOLATION_CODE);
    let summary = delete_permanently_guarded(&items);
    assert_eq!(summary.failures[0].code, VIOLATION_CODE);
    assert!(Path::new(&w.a()).exists());
    let err = create_guarded(&s(w.u.path()), "x", "file").unwrap_err();
    assert!(err.starts_with(&format!("code={VIOLATION_CODE}")), "{err}");
    assert!(!w.u.path().join("x").exists());
}

// The scope is a per-thread override restored by its guard: after the guard drops, no scope remains.
#[test]
fn ac88_the_scope_override_does_not_outlive_its_guard() {
    let w = world();
    {
        let _scope = with_scope_on(&w);
    }
    let summary = copy(&[w.a()], &w.dest(), &Probe::new());
    assert_eq!((summary.ok, summary.failed), (1, 0), "{summary:?}");
}
