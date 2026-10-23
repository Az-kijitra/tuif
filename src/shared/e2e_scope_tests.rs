//! TDD (unit-test-writer, Phase 11, 2026-10-03): the debug-only E2E file scope.
//! Specs: SPEC-0400-0001 R-10, R-10-1 (invalid value / missing folder = start-up
//! error, never created), R-10-3 (canonical, component-wise, undecidable = reject),
//! R-10-5 (OS-association launch always rejected), R-10-6 (sources and link
//! targets are judged too, canonicalize failure = reject), AC-10;
//! SPEC-0200-0001 §3.10.17.6 / §3.10.17.6-2 (MUST-123, SHOULD-178..180).
//!
//! No `env::set_var` anywhere: `resolve_scope` takes the raw value and `Scope`
//! is injected, so nothing depends on the process-wide `OnceLock`.
//! Tests that need a junction / symlink print `SKIP: ...` (with the reason) and
//! return when the link cannot be created in this environment.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::e2e_scope::{
    check_container, check_entry, check_external_launch, check_source, resolve_scope, Scope,
    ScopeViolation, VIOLATION_CODE,
};

struct Env {
    _base: tempfile::TempDir,
    /// canonical scope root `T`
    root: PathBuf,
    /// a sibling folder outside the scope
    outside: PathBuf,
    /// a sibling whose name starts with the root's name (`root-evil`)
    evil: PathBuf,
    scope: Scope,
}

fn canon(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|e| panic!("canonicalize {}: {e}", p.display()))
}

fn env() -> Env {
    let base = tempfile::tempdir().expect("tempdir");
    let root = base.path().join("root");
    let outside = base.path().join("outside");
    let evil = base.path().join("root-evil");
    for d in [&root, &outside, &evil] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(root.join("in.txt"), "in").unwrap();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(outside.join("out.txt"), "out").unwrap();
    fs::write(evil.join("e.txt"), "e").unwrap();
    let root = canon(&root);
    let scope = Scope::new(root.clone());
    Env {
        _base: base,
        root,
        outside: canon(&outside),
        evil: canon(&evil),
        scope,
    }
}

/// NTFS junction (no elevation) on Windows, symlink elsewhere.
fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

fn remove_dir_link(link: &Path) {
    #[cfg(windows)]
    {
        let _ = fs::remove_dir(link);
    }
    #[cfg(unix)]
    {
        let _ = fs::remove_file(link);
    }
}

fn make_file_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

/// `base` + `rest` (written with '/') joined TEXTUALLY. On Windows `PathBuf::join` onto a verbatim
/// path (the canonical form) normalises "." and ".." away, which would silently turn the
/// "reject a .. component" cases below into plain inside paths: the scope must see the raw text.
fn textual(base: &Path, rest: &str) -> PathBuf {
    let sep = std::path::MAIN_SEPARATOR.to_string();
    PathBuf::from(format!(
        "{}{}{}",
        base.display(),
        sep,
        rest.replace('/', &sep)
    ))
}

fn skip(reason: &str) {
    eprintln!("SKIP: {reason}");
}

fn ok(r: Result<(), ScopeViolation>, what: &str) {
    assert!(r.is_ok(), "{what} must be allowed, got {r:?}");
}

fn denied(r: Result<(), ScopeViolation>, what: &str) {
    assert!(r.is_err(), "{what} must be rejected");
}

// ============================================================== resolve_scope (R-10-1)

#[test]
fn resolve_scope_unset_means_the_scope_is_off() {
    assert_eq!(resolve_scope(None).unwrap(), None);
}

#[test]
fn resolve_scope_rejects_empty_and_blank_values() {
    for raw in ["", " ", "   ", "\t", " \t "] {
        let r = resolve_scope(Some(std::ffi::OsStr::new(raw)));
        assert!(
            r.is_err(),
            "{raw:?} must be a start-up error (never 'scope off')"
        );
    }
}

#[test]
fn resolve_scope_rejects_a_non_unicode_value() {
    #[cfg(windows)]
    let raw = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0x0043, 0x003A, 0x005C, 0xD800, 0x0041])
    };
    #[cfg(unix)]
    let raw = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![b'/', 0xFF, 0xFE, b'a'])
    };
    assert!(resolve_scope(Some(raw.as_os_str())).is_err());
}

#[test]
fn resolve_scope_rejects_relative_paths() {
    for raw in ["relative", r"rel\dir", "./x", "."] {
        assert!(
            resolve_scope(Some(std::ffi::OsStr::new(raw))).is_err(),
            "{raw}"
        );
    }
}

#[test]
fn resolve_scope_rejects_parent_components() {
    let e = env();
    let with_dotdot = textual(&e.root, "sub/..");
    assert!(resolve_scope(Some(with_dotdot.as_os_str())).is_err());
}

// R-10-1 (post-approval): a missing folder is an error AND is not created (unlike R-06-3).
#[test]
fn resolve_scope_rejects_a_missing_folder_without_creating_it() {
    let e = env();
    let missing = e.root.join("not-yet");
    assert!(resolve_scope(Some(missing.as_os_str())).is_err());
    assert!(!missing.exists(), "the folder must NOT be created");
}

#[test]
fn resolve_scope_rejects_an_existing_file() {
    let e = env();
    let file = e.root.join("in.txt");
    assert!(resolve_scope(Some(file.as_os_str())).is_err());
}

#[test]
fn resolve_scope_accepts_an_existing_absolute_directory_and_yields_its_canonical_path() {
    let e = env();
    let got = resolve_scope(Some(e.root.as_os_str()))
        .unwrap()
        .expect("scope on");
    assert_eq!(canon(&got), e.root);
}

// ============================================================== constants / error text

#[test]
fn a_violation_is_expressed_as_code_equals_e2e_scope_violation() {
    assert_eq!(VIOLATION_CODE, "e2e_scope_violation");
    let e = ScopeViolation("outside the scope: X".to_string()).to_command_error();
    assert!(e.starts_with("code=e2e_scope_violation; "), "{e}");
}

// ============================================================== check_container

#[test]
fn container_inside_the_scope_is_allowed() {
    let e = env();
    ok(e.scope.check_container(&e.root), "the root itself");
    ok(
        e.scope.check_container(&e.root.join("sub")),
        "an existing subfolder",
    );
}

// SHOULD-178 / §3.10.17.6: a missing output folder is judged by its deepest existing ancestor.
#[test]
fn container_that_does_not_exist_yet_is_judged_by_the_deepest_existing_ancestor() {
    let e = env();
    ok(
        e.scope
            .check_container(&e.root.join("new").join("deeper").join("out")),
        "missing under the root",
    );
    denied(
        e.scope.check_container(&e.outside.join("new").join("out")),
        "missing under an outside folder",
    );
}

#[test]
fn container_outside_the_scope_is_rejected() {
    let e = env();
    denied(e.scope.check_container(&e.outside), "a sibling folder");
    denied(
        e.scope.check_container(e.root.parent().unwrap()),
        "the parent of the root",
    );
}

// component-wise comparison: `root` must not match `root-evil`.
#[test]
fn container_with_the_same_name_prefix_is_a_different_folder() {
    let e = env();
    denied(e.scope.check_container(&e.evil), "root-evil");
    denied(
        e.scope.check_container(&e.evil.join("sub")),
        "root-evil\\sub",
    );
}

#[test]
fn container_with_parent_components_is_rejected_even_when_it_would_resolve_inside() {
    let e = env();
    denied(
        e.scope.check_container(&textual(&e.root, "sub/../sub")),
        "inside via ..",
    );
    denied(
        e.scope
            .check_container(&textual(&e.root, "new/../../outside")),
        "escape via ..",
    );
    denied(e.scope.check_container(&textual(&e.root, "..")), "root\\..");
}

#[test]
fn container_with_dot_components_or_dots_and_spaces_only_names_in_the_missing_part_is_rejected() {
    let e = env();
    denied(
        e.scope.check_container(&textual(&e.root, "new/./x")),
        "a `.` component",
    );
    denied(
        e.scope.check_container(&e.root.join("new").join("...")),
        "dots only",
    );
    denied(
        e.scope.check_container(&e.root.join("new").join(". .")),
        "dots and spaces only",
    );
    denied(
        e.scope.check_container(&e.root.join("new").join(" ")),
        "spaces only",
    );
}

#[cfg(windows)]
#[test]
fn container_with_a_trailing_space_or_dot_in_the_missing_part_is_rejected_win32_trims_them() {
    let e = env();
    denied(
        e.scope.check_container(&e.root.join("new ").join("x")),
        "trailing space",
    );
    denied(
        e.scope.check_container(&e.root.join("new.").join("x")),
        "trailing dot",
    );
    ok(
        e.scope
            .check_container(&e.root.join("new dir.v2").join("x")),
        "inner space and dot are fine",
    );
}

#[cfg(windows)]
#[test]
fn container_comparison_ignores_case_and_accepts_the_extended_prefix_form() {
    let e = env();
    let s = e.root.join("sub").to_string_lossy().into_owned();
    ok(
        e.scope.check_container(Path::new(&s.to_uppercase())),
        "upper-cased path",
    );
    ok(
        e.scope.check_container(Path::new(&s.to_lowercase())),
        "lower-cased path",
    );
    // the canonical form carries \\?\ ; the plain form must compare equal after canonicalize
    let plain = s.strip_prefix(r"\\?\").unwrap_or(&s).to_string();
    ok(
        e.scope.check_container(Path::new(&plain)),
        "plain drive form",
    );
    ok(e.scope.check_container(Path::new(&s)), r"\\?\ form");
}

#[test]
fn container_that_is_empty_or_relative_is_rejected() {
    let e = env();
    denied(e.scope.check_container(Path::new("")), "empty");
    denied(e.scope.check_container(Path::new("rel-out")), "relative");
}

#[test]
fn container_reached_through_a_link_that_leaves_the_scope_is_rejected() {
    let e = env();
    let link = e.root.join("escape");
    if !make_dir_link(&link, &e.outside) {
        return skip("cannot create a junction/symlink here (check_container link escape)");
    }
    denied(
        e.scope.check_container(&link),
        "the link itself as a destination",
    );
    denied(
        e.scope.check_container(&link.join("missing").join("x")),
        "a missing folder below the link",
    );
    remove_dir_link(&link);
}

// ============================================================== check_entry

#[test]
fn entry_inside_is_allowed_whether_or_not_it_exists() {
    let e = env();
    ok(
        e.scope.check_entry(&e.root.join("in.txt")),
        "an existing file",
    );
    ok(
        e.scope.check_entry(&e.root.join("sub")),
        "an existing folder",
    );
    ok(
        e.scope.check_entry(&e.root.join("will-be-created.txt")),
        "a missing item in an existing folder",
    );
}

#[test]
fn entry_outside_is_rejected() {
    let e = env();
    denied(
        e.scope.check_entry(&e.outside.join("out.txt")),
        "outside file",
    );
    denied(
        e.scope.check_entry(&e.evil.join("e.txt")),
        "same-prefix sibling",
    );
}

#[test]
fn entry_whose_parent_cannot_be_canonicalized_is_rejected() {
    let e = env();
    denied(
        e.scope
            .check_entry(&e.root.join("no-such-dir").join("f.txt")),
        "missing parent",
    );
}

#[test]
fn entry_names_that_are_parent_dot_or_empty_are_rejected() {
    let e = env();
    denied(e.scope.check_entry(&textual(&e.root, "..")), "..");
    denied(
        e.scope.check_entry(&textual(&e.root, "sub/../in.txt")),
        ".. in the middle",
    );
    denied(e.scope.check_entry(&textual(&e.root, ".")), ".");
    denied(e.scope.check_entry(Path::new("")), "empty");
}

#[cfg(windows)]
#[test]
fn entry_name_with_trailing_space_or_dot_is_rejected() {
    let e = env();
    denied(
        e.scope.check_entry(&e.root.join("in.txt ")),
        "trailing space",
    );
    denied(e.scope.check_entry(&e.root.join("in.txt.")), "trailing dot");
}

// The link ITSELF is the item for delete / rename / move (not followed); its parent decides.
#[test]
fn entry_that_is_a_link_pointing_outside_is_judged_by_the_link_itself() {
    let e = env();
    let link = e.root.join("escape");
    if !make_dir_link(&link, &e.outside) {
        return skip("cannot create a junction/symlink here (check_entry on a link)");
    }
    ok(
        e.scope.check_entry(&link),
        "deleting/renaming the link itself stays inside",
    );
    denied(
        e.scope.check_entry(&link.join("out.txt")),
        "an item reached THROUGH the link (parent resolves outside)",
    );
    remove_dir_link(&link);
}

// ============================================================== check_source (MUST-123)

#[test]
fn source_inside_without_links_is_allowed() {
    let e = env();
    ok(e.scope.check_source(&e.root.join("in.txt")), "file");
    ok(e.scope.check_source(&e.root.join("sub")), "folder");
}

#[test]
fn source_outside_is_rejected() {
    let e = env();
    denied(
        e.scope.check_source(&e.outside.join("out.txt")),
        "outside file",
    );
    denied(
        e.scope.check_source(&e.evil.join("e.txt")),
        "same-prefix sibling",
    );
}

// R-10-6: cannot be canonicalized (e.g. does not exist) = cannot be judged = reject.
#[test]
fn source_that_cannot_be_canonicalized_is_rejected() {
    let e = env();
    denied(
        e.scope.check_source(&e.root.join("missing.txt")),
        "missing file",
    );
    denied(
        e.scope
            .check_source(&e.root.join("no-such-dir").join("f.txt")),
        "missing parent",
    );
    denied(e.scope.check_source(Path::new("")), "empty");
}

#[test]
fn source_that_is_a_link_leaving_the_scope_is_rejected_but_a_link_staying_inside_is_allowed() {
    let e = env();
    let esc = e.root.join("escape");
    let stay = e.root.join("stay");
    if !make_dir_link(&esc, &e.outside) {
        return skip("cannot create a junction/symlink here (check_source link escape)");
    }
    denied(
        e.scope.check_source(&esc),
        "folder link to the outside (copy / zip source)",
    );
    denied(
        e.scope.check_source(&esc.join("out.txt")),
        "file below a link to the outside",
    );
    if make_dir_link(&stay, &e.root.join("sub")) {
        ok(
            e.scope.check_source(&stay),
            "a link that resolves inside the scope",
        );
        remove_dir_link(&stay);
    } else {
        skip("second junction could not be created (inside-link case)");
    }
    remove_dir_link(&esc);
}

#[test]
fn source_that_is_a_file_symlink_to_the_outside_is_rejected() {
    let e = env();
    let link = e.root.join("out-link.txt");
    if !make_file_link(&link, &e.outside.join("out.txt")) {
        return skip(
            "cannot create a file symlink here (needs Developer Mode / privilege on Windows)",
        );
    }
    denied(e.scope.check_source(&link), "file symlink to the outside");
    let _ = fs::remove_file(&link);
}

// ============================================================== external launch (R-10-5 / SHOULD-179)

#[test]
fn external_launch_is_always_rejected_while_the_scope_is_active() {
    let e = env();
    denied(
        e.scope.check_external_launch(&e.root.join("in.txt")),
        "a file inside the scope",
    );
    denied(e.scope.check_external_launch(&e.root), "the root");
    denied(
        e.scope.check_external_launch(&e.root.join("missing.lnk")),
        "a missing shortcut inside",
    );
    denied(
        e.scope.check_external_launch(&e.outside.join("out.txt")),
        "outside",
    );
    denied(e.scope.check_external_launch(Path::new("")), "empty");
}

// ============================================================== no scope set

// The process-wide scope is never initialised by the tests (no env var is set), so the free
// functions model "scope off": everything is allowed (release builds: always).
#[test]
fn without_an_active_scope_every_check_allows_everything() {
    let anywhere = Path::new(if cfg!(windows) {
        r"Z:\anywhere\at\all"
    } else {
        "/anywhere/at/all"
    });
    ok(check_container(anywhere), "check_container");
    ok(check_entry(anywhere), "check_entry");
    ok(check_source(anywhere), "check_source");
    ok(
        check_external_launch(anywhere),
        "check_external_launch (only active scopes reject)",
    );
}

// ============================================================== test override (AC-88 support)

// SPEC-0200-0001 §3.12.9 AC-88 / MINOR-11: `set_test_scope` activates a scope for the free
// `check_*` fns of THIS thread; `ScopeGuard::drop` always restores the previous value.
#[test]
fn set_test_scope_activates_the_scope_for_the_free_check_fns_until_the_guard_drops() {
    let e = env();
    let outside_file = e.outside.join("out.txt");
    {
        let _guard = super::e2e_scope::set_test_scope(e.scope.clone());
        denied(
            check_container(&e.outside),
            "outside container while the override is active",
        );
        denied(
            check_source(&outside_file),
            "outside source while the override is active",
        );
        denied(
            check_external_launch(&e.root),
            "launch is always refused while a scope is active",
        );
        ok(check_container(&e.root), "the scope root itself");
    }
    // dropped: back to "scope off"
    ok(check_container(&e.outside), "after the guard dropped");
    ok(
        check_external_launch(&e.root),
        "after the guard dropped (launch)",
    );
}

#[test]
fn nested_test_scopes_restore_the_previous_one_in_reverse_order() {
    let a = env();
    let b = env();
    let _outer = super::e2e_scope::set_test_scope(a.scope.clone());
    {
        let _inner = super::e2e_scope::set_test_scope(b.scope.clone());
        ok(check_container(&b.root), "inner scope root");
        denied(
            check_container(&a.root),
            "outer root is outside the inner scope",
        );
    }
    ok(
        check_container(&a.root),
        "outer scope is back after the inner guard dropped",
    );
    denied(
        check_container(&b.root),
        "inner root is outside the outer scope",
    );
}

#[test]
fn a_test_scope_does_not_leak_to_other_threads() {
    let e = env();
    let _guard = super::e2e_scope::set_test_scope(e.scope.clone());
    let outside = e.outside.clone();
    let seen_by_other_thread = std::thread::spawn(move || check_container(&outside).is_ok())
        .join()
        .unwrap();
    assert!(
        seen_by_other_thread,
        "another thread has no override (scope off)"
    );
}
