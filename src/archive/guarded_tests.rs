//! TDD (unit-test-writer, 2026-10-07): the inner layer of the `archive_*` commands refuses every
//! non-absolute path argument (and every non-absolute RESOLVED output / destination) before touching
//! the file system. Specs: SPEC-0300-0014 第5版 R-322-1 / R-322-3 / R-341-1 / R-341-2 / R-395-5,
//! AC-387 / AC-388 / AC-389 / AC-390 / AC-391; SPEC-0300-0011 第9版 R-89-1 #11..#14 / R-89-4 / R-89-5 /
//! T-10 / T-13 / T-14, AC-88; SPEC-0200-0001 §3.12.3 / §3.12.5 (SHOULD-188 `no_items` first).
//!
//! The working directory is never changed (parallel tests): relative paths are random names
//! `zz-abs-path-<unique>`; afterwards U (the source folder) and the working directory must show no output,
//! no temp file and no extracted entry.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::create::CreateHooks;
use super::error::ArchiveErrorDto;
use super::extract::ExtractHooks;
use super::guarded::{check_create_guarded, create_guarded, detect_conflicts_guarded, extract_guarded};
use super::test_support::{env_plenty, tempdir, tree_names, write_file};
use super::zip_fixture::ZipBuilder;
use crate::shared::e2e_scope::{set_test_scope, Scope, VIOLATION_CODE};

const INVALID: &str = "invalid_path";

// ------------------------------------------------------------------------------------ helpers

fn rand_name() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("zz-abs-path-{nanos}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

fn cwd_drive() -> char {
    let cwd = std::env::current_dir().expect("cwd").to_string_lossy().trim_start_matches(r"\\?\").to_string();
    cwd.chars().next().filter(|c| c.is_ascii_alphabetic()).unwrap_or('C')
}

/// Removes the random relative names from the working directory at the end of the test.
struct Debris(Vec<String>);
impl Drop for Debris {
    fn drop(&mut self) {
        for name in &self.0 {
            let _ = fs::remove_dir_all(name).or_else(|_| fs::remove_file(name));
        }
    }
}

/// Relative (i), drive-relative (ii), root-relative (iii), empty (iv) and `/`-style forms.
fn bad_forms(rand: &str) -> Vec<String> {
    let d = cwd_drive();
    vec![
        rand.to_string(),
        "a.txt".to_string(),
        format!(r".\{rand}"),
        format!("{d}:{rand}"),
        format!("{d}:"),
        format!(r"\{rand}"),
        format!(r"\{rand}\a.txt"),
        String::new(),
        format!("{d}:/{rand}"),
    ]
}

fn code_of<T>(result: Result<T, ArchiveErrorDto>) -> String {
    match result {
        Ok(_) => "<ok>".to_string(),
        Err(e) => e.code,
    }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// U: `a.txt`, folder `d/`, folder `sub/`; a ready ZIP `z.zip` (entry `x.txt`) lives in V (another folder).
struct World {
    u: tempfile::TempDir,
    v: tempfile::TempDir,
    zip: PathBuf,
    rand: String,
    _debris: Debris,
}

fn world() -> World {
    let u = tempdir();
    let v = tempdir();
    write_file(u.path(), "a.txt", b"A");
    write_file(u.path(), "d/inner.txt", b"I");
    fs::create_dir(u.path().join("sub")).unwrap();
    let zip = ZipBuilder::new().file("x.txt", b"X").write_to(v.path(), "z.zip");
    let rand = rand_name();
    let debris = Debris(vec![rand.clone()]);
    World { u, v, zip, rand, _debris: debris }
}

impl World {
    fn base(&self) -> String {
        s(self.u.path())
    }
    fn a(&self) -> String {
        s(&self.u.path().join("a.txt"))
    }
    fn zip(&self) -> String {
        s(&self.zip)
    }
    /// What must not change on a refusal: the names under U and V, and the working directory's random name.
    fn fingerprint(&self) -> (Vec<String>, Vec<String>) {
        (tree_names(self.u.path()), tree_names(self.v.path()))
    }
    fn assert_untouched(&self, before: &(Vec<String>, Vec<String>), what: &str) {
        assert_eq!(&self.fingerprint(), before, "{what}: U / V changed (output, temp file or extracted entry)");
        assert!(!Path::new(&self.rand).exists(), "{what}: the random relative name appeared in the working directory");
    }
}

fn check(w: &World, items: &[String], output: &str, base: &str) -> String {
    let _ = w;
    code_of(check_create_guarded(items, output, base))
}

fn create(items: &[String], output: &str, base: &str) -> String {
    code_of(create_guarded(items, output, base, false, &CreateHooks::none()))
}

fn detect(zip: &str, dest: &str, base: &str) -> String {
    code_of(detect_conflicts_guarded(zip, dest, base, &env_plenty()))
}

fn extract(zip: &str, dest: &str, base: &str) -> String {
    code_of(extract_guarded(zip, dest, base, &env_plenty(), &HashMap::new(), &ExtractHooks::none()))
}

// ------------------------------------------------------------------------------------ AC-387 (create / check_create)

#[test]
fn ac387_a_relative_items_entry_is_invalid_path_for_check_and_create() {
    let w = world();
    let before = w.fingerprint();
    for bad in bad_forms(&w.rand) {
        for items in [vec![bad.clone()], vec![w.a(), bad.clone()]] {
            assert_eq!(check(&w, &items, "out.zip", &w.base()), INVALID, "check items {items:?}");
            assert_eq!(create(&items, "out.zip", &w.base()), INVALID, "create items {items:?}");
        }
    }
    w.assert_untouched(&before, "AC-387 items");
}

#[test]
fn ac387_a_relative_drive_relative_or_root_relative_base_dir_is_invalid_path() {
    let w = world();
    let before = w.fingerprint();
    for bad in bad_forms(&w.rand) {
        assert_eq!(check(&w, &[w.a()], "out.zip", &bad), INVALID, "check base_dir {bad:?}");
        assert_eq!(create(&[w.a()], "out.zip", &bad), INVALID, "create base_dir {bad:?}");
        // an absolute output name does not excuse a bad base_dir either (the base_dir is always checked)
        assert_eq!(create(&[w.a()], &format!(r"{}\out.zip", w.base()), &bad), INVALID, "abs output, base_dir {bad:?}");
    }
    w.assert_untouched(&before, "AC-387 base_dir");
}

// SHOULD-188: `no_items` comes first, before the absolute-path check and before anything else.
#[test]
fn should188_no_items_wins_over_every_path_problem() {
    let w = world();
    for bad_base in bad_forms(&w.rand) {
        assert_eq!(check(&w, &[], "out.zip", &bad_base), "no_items", "check, base_dir {bad_base:?}");
        assert_eq!(create(&[], "out.zip", &bad_base), "no_items", "create, base_dir {bad_base:?}");
    }
    assert_eq!(check(&w, &[], r"C:out", &w.base()), "no_items");
}

// Regression: absolute inputs still work (and `not_found` for a missing absolute item is kept).
#[test]
fn ac387_absolute_inputs_still_create_the_zip() {
    let w = world();
    let result = create_guarded(&[w.a()], "out.zip", &w.base(), false, &CreateHooks::none()).expect("absolute inputs");
    assert_eq!(result.output_path, w.u.path().join("out.zip"));
    assert!(w.u.path().join("out.zip").is_file());
}

#[test]
fn ac387_a_missing_absolute_item_is_still_not_found() {
    let w = world();
    let missing = s(&w.u.path().join("nope.txt"));
    assert_eq!(check(&w, &[missing.clone()], "out.zip", &w.base()), "not_found");
    assert_eq!(create(&[missing], "out.zip", &w.base()), "not_found");
}

// ------------------------------------------------------------------------------------ AC-388 (extract / detect, T-10)

#[test]
fn ac388_a_relative_zip_path_is_invalid_path_and_nothing_is_extracted() {
    let w = world();
    let dest = s(&w.u.path().join("out"));
    let before = w.fingerprint();
    for bad in bad_forms(&w.rand) {
        assert_eq!(detect(&bad, &dest, &w.base()), INVALID, "detect zip {bad:?}");
        assert_eq!(extract(&bad, &dest, &w.base()), INVALID, "extract zip {bad:?}");
    }
    w.assert_untouched(&before, "AC-388 zip_path");
    assert!(!w.u.path().join("out").exists(), "the destination folder was not created");
}

// T-10: base_dir is checked ALWAYS, also when the destination is absolute and does not need it.
#[test]
fn ac388_a_bad_base_dir_is_invalid_path_even_with_an_absolute_destination() {
    let w = world();
    let dest = s(&w.u.path().join("out"));
    let before = w.fingerprint();
    for bad in bad_forms(&w.rand) {
        assert_eq!(detect(&w.zip(), &dest, &bad), INVALID, "detect base_dir {bad:?}");
        assert_eq!(extract(&w.zip(), &dest, &bad), INVALID, "extract base_dir {bad:?}");
    }
    w.assert_untouched(&before, "AC-388 base_dir");
    assert!(!w.u.path().join("out").exists());
}

#[test]
fn ac388_absolute_inputs_still_detect_and_extract() {
    let w = world();
    let dest = s(&w.u.path().join("out"));
    let plan = detect_conflicts_guarded(&w.zip(), &dest, &w.base(), &env_plenty()).expect("detect");
    assert_eq!(plan.total_entries, 1);
    assert!(plan.conflicts.is_empty());
    let result = extract_guarded(&w.zip(), &dest, &w.base(), &env_plenty(), &HashMap::new(), &ExtractHooks::none()).expect("extract");
    assert_eq!(result.extracted, 1);
    assert_eq!(fs::read_to_string(w.u.path().join("out").join("x.txt")).unwrap(), "X");
}

// ------------------------------------------------------------------------------------ AC-389 (relative INPUT names still work)

#[test]
fn ac389_a_relative_output_name_is_resolved_under_base_dir() {
    let w = world();
    let checked = check_create_guarded(&[w.a()], "out", &w.base()).expect("relative output name");
    assert_eq!(checked.output_path, w.u.path().join("out.zip"));
    create_guarded(&[w.a()], "made", &w.base(), false, &CreateHooks::none()).expect("create");
    assert!(w.u.path().join("made.zip").is_file());
}

#[test]
fn ac389_a_drive_relative_or_root_relative_output_is_invalid_path() {
    let w = world();
    let before = w.fingerprint();
    let d = cwd_drive();
    for bad in [format!("{d}:out"), format!("{d}:out.zip"), r"\out".to_string(), r"\out.zip".to_string()] {
        assert_eq!(check(&w, &[w.a()], &bad, &w.base()), INVALID, "check {bad:?}");
        assert_eq!(create(&[w.a()], &bad, &w.base()), INVALID, "create {bad:?}");
    }
    w.assert_untouched(&before, "AC-389 output");
}

// T-14: `sub/out.zip` works (`/` -> `\` before resolving) -> U\sub\out.zip
#[test]
fn ac389_a_relative_output_with_a_slash_works_via_t14() {
    let w = world();
    let checked = check_create_guarded(&[w.a()], "sub/out.zip", &w.base()).expect("sub/out.zip");
    assert_eq!(checked.output_path, w.u.path().join("sub").join("out.zip"));
    create_guarded(&[w.a()], "sub/made.zip", &w.base(), false, &CreateHooks::none()).expect("create sub/made.zip");
    assert!(w.u.path().join("sub").join("made.zip").is_file());
}

#[test]
fn ac389_a_relative_destination_is_resolved_under_base_dir_and_a_slash_destination_works() {
    let w = world();
    let plan = detect_conflicts_guarded(&w.zip(), "out", &w.base(), &env_plenty()).expect("relative destination");
    let u_name = w.u.path().file_name().unwrap();
    assert!(plan.destination.ends_with(Path::new(u_name).join("out")), "{}", plan.destination.display());
    let result = extract_guarded(&w.zip(), "sub/x", &w.base(), &env_plenty(), &HashMap::new(), &ExtractHooks::none())
        .expect("sub/x via T-14");
    assert_eq!(result.extracted, 1);
    assert!(w.u.path().join("sub").join("x").join("x.txt").is_file());
}

#[test]
fn ac389_a_drive_relative_or_root_relative_destination_is_invalid_path() {
    let w = world();
    let before = w.fingerprint();
    let d = cwd_drive();
    for bad in [format!("{d}:out"), format!("{d}:"), r"\out".to_string(), String::new()] {
        assert_eq!(detect(&w.zip(), &bad, &w.base()), INVALID, "detect {bad:?}");
        assert_eq!(extract(&w.zip(), &bad, &w.base()), INVALID, "extract {bad:?}");
    }
    w.assert_untouched(&before, "AC-389 destination");
}

// ------------------------------------------------------------------------------------ AC-390 (T-13: the RESOLVED path)

#[test]
fn ac390_device_style_and_shareless_unc_outputs_are_invalid_path() {
    let w = world();
    let before = w.fingerprint();
    for bad in [r"\\.\pipe\x.zip", "//server", r"\\server"] {
        assert_eq!(check(&w, &[w.a()], bad, &w.base()), INVALID, "check {bad:?}");
        assert_eq!(create(&[w.a()], bad, &w.base()), INVALID, "create {bad:?}");
    }
    w.assert_untouched(&before, "AC-390 output");
}

#[test]
fn ac390_device_style_and_shareless_unc_destinations_are_invalid_path() {
    let w = world();
    let before = w.fingerprint();
    for bad in [r"\\.\PhysicalDrive0", r"\\.\pipe\x", r"\\server", "//server", r"\\server\"] {
        assert_eq!(detect(&w.zip(), bad, &w.base()), INVALID, "detect {bad:?}");
        assert_eq!(extract(&w.zip(), bad, &w.base()), INVALID, "extract {bad:?}");
    }
    w.assert_untouched(&before, "AC-390 destination");
}

// R-89-4 + T-13: with an E2E scope active the code is still `invalid_path` (judged before the scope check).
#[test]
fn ac390_with_a_scope_active_the_resolved_check_still_comes_first() {
    let w = world();
    let _scope = set_test_scope(Scope::new(fs::canonicalize(w.v.path()).unwrap()));
    assert_eq!(create(&[w.a()], r"\\.\pipe\x.zip", &w.base()), INVALID);
    assert_eq!(extract(&w.zip(), r"\\.\PhysicalDrive0", &w.base()), INVALID);
    assert_eq!(extract(&w.zip(), "//server", &w.base()), INVALID);
}

// ------------------------------------------------------------------------------------ AC-391 (T-14: `/` -> `\`)

#[test]
fn ac391_an_absolute_output_written_with_slashes_is_created_at_the_backslash_path() {
    let w = world();
    let slashed = format!("{}/out.zip", w.base().replace('\\', "/"));
    assert!(slashed.contains('/') && !slashed.contains(r"\\?\"));
    let result = create_guarded(&[w.a()], &slashed, &w.base(), false, &CreateHooks::none()).expect("D:/backup/out.zip style");
    assert_eq!(result.output_path, w.u.path().join("out.zip"), "reported as the backslash path");
    assert!(w.u.path().join("out.zip").is_file());
    let checked = check_create_guarded(&[w.a()], &slashed, &w.base()).expect("check agrees (now overwrite)");
    assert_eq!(checked.output_path, w.u.path().join("out.zip"));
    assert!(checked.overwrite_required, "the file just created is found at the same path");
}

#[test]
fn ac391_an_absolute_destination_written_with_slashes_extracts_there() {
    let w = world();
    let slashed = format!("{}/viaslash", w.base().replace('\\', "/"));
    let result = extract_guarded(&w.zip(), &slashed, &w.base(), &env_plenty(), &HashMap::new(), &ExtractHooks::none()).expect("slash destination");
    assert_eq!(result.extracted, 1);
    assert!(w.u.path().join("viaslash").join("x.txt").is_file());
}

// T-14 applies ONLY to the output / destination input: items, zip_path and base_dir are judged as given
// (`C:/foo` is rejected by T-02), so a `/` there is `invalid_path`.
#[test]
fn ac391_slashes_in_items_zip_path_or_base_dir_are_not_converted_and_are_refused() {
    let w = world();
    let before = w.fingerprint();
    let slashed_a = w.a().replace('\\', "/");
    let slashed_base = w.base().replace('\\', "/");
    let slashed_zip = w.zip().replace('\\', "/");
    assert_eq!(create(&[slashed_a.clone()], "out.zip", &w.base()), INVALID, "items");
    assert_eq!(check(&w, &[slashed_a], "out.zip", &w.base()), INVALID, "items (check)");
    assert_eq!(create(&[w.a()], "out.zip", &slashed_base), INVALID, "base_dir (create)");
    assert_eq!(check(&w, &[w.a()], "out.zip", &slashed_base), INVALID, "base_dir (check)");
    let dest = s(&w.u.path().join("o"));
    assert_eq!(extract(&slashed_zip, &dest, &w.base()), INVALID, "zip_path");
    assert_eq!(detect(&w.zip(), &dest, &slashed_base), INVALID, "base_dir (detect)");
    w.assert_untouched(&before, "AC-391 not converted");
}

// ------------------------------------------------------------------------------------ AC-88 (before the E2E scope)

#[test]
fn ac88_with_a_scope_active_relative_inputs_are_invalid_path_not_a_scope_violation() {
    let w = world();
    let _scope = set_test_scope(Scope::new(fs::canonicalize(w.v.path()).unwrap())); // U is OUTSIDE the scope
    let rel = w.rand.clone();
    assert_eq!(create(&[rel.clone()], "out.zip", &w.base()), INVALID, "create items");
    assert_eq!(create(&[w.a()], "out.zip", &rel), INVALID, "create base_dir");
    assert_eq!(check(&w, &[rel.clone()], "out.zip", &w.base()), INVALID, "check items");
    assert_eq!(extract(&rel, &s(&w.v.path().join("o")), &w.base()), INVALID, "extract zip_path");
    assert_eq!(extract(&w.zip(), &s(&w.v.path().join("o")), &rel), INVALID, "extract base_dir");
    assert_eq!(detect(&rel, &s(&w.v.path().join("o")), &w.base()), INVALID, "detect zip_path");
    assert!(!Path::new(&rel).exists());
}

// Control: the scope really is active (absolute inputs outside it -> `e2e_scope_violation`), and the
// side-effect-free checks (`check` / `detect`) have NO scope check (as before).
#[test]
fn ac88_control_absolute_inputs_outside_the_scope_are_violations_but_check_and_detect_are_not_scoped() {
    let w = world();
    let _scope = set_test_scope(Scope::new(fs::canonicalize(w.v.path()).unwrap()));
    assert_eq!(create(&[w.a()], "out.zip", &w.base()), VIOLATION_CODE, "create: U is outside the scope");
    assert!(!w.u.path().join("out.zip").exists());
    let inside_dest = s(&w.v.path().join("o"));
    assert_eq!(extract(&w.zip(), &inside_dest, &w.base()), "<ok>", "everything inside the scope (V) works");
    let outside_dest = s(&w.u.path().join("o"));
    assert_eq!(extract(&w.zip(), &outside_dest, &w.base()), VIOLATION_CODE, "destination outside the scope");
    assert!(!w.u.path().join("o").exists());
    assert_eq!(check(&w, &[w.a()], "out.zip", &w.base()), "<ok>", "check_create has no scope check");
    assert_eq!(detect(&w.zip(), &outside_dest, &w.base()), "<ok>", "detect has no scope check");
}
