//! TDD (unit-test-writer, 2026-09-27): tests for `shared::identity`
//! (`same_entity`, `is_strictly_within`, `normalize_allowing_missing`).
//!
//! Specs: SPEC-0300-0014 R-325-1, R-343-2, R-367-1, AC-321-4, AC-369-1;
//! SPEC-0300-0011 R-37-1 (MUST-59); SPEC-0200-0001 §3.4.13-1 (MUST-65),
//! §3.4.13-5 (MUST-69: the `normalize_allowing_missing` contract).

use super::identity::{is_strictly_within, normalize_allowing_missing, same_entity};
use std::fs;
use std::path::{Path, PathBuf};

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn canon(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|e| panic!("canonicalize {}: {e}", p.display()))
}

/// NTFS junction (no elevation) on Windows, symlink elsewhere. `false` when it
/// could not be created (the test then reports a skip and returns).
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

// ---------------------------------------------------------------- same_entity

// R-37-1: identical path -> same entity.
#[test]
fn same_entity_is_true_for_the_same_path() {
    let tmp = tempdir();
    let f = tmp.path().join("a.txt");
    fs::write(&f, "x").unwrap();
    assert_eq!(same_entity(&f, &f), Some(true));
}

// R-37-1: different files -> `Some(false)`, not `None`.
#[test]
fn same_entity_is_false_for_different_files() {
    let tmp = tempdir();
    let a = tmp.path().join("a.txt");
    let b = tmp.path().join("b.txt");
    fs::write(&a, "x").unwrap();
    fs::write(&b, "x").unwrap();
    assert_eq!(same_entity(&a, &b), Some(false));
}

// R-325-1 / R-367-1 / AC-369-1: letter-case alias of the same entry (Windows).
#[cfg(windows)]
#[test]
fn same_entity_sees_through_letter_case_aliases_on_windows() {
    let tmp = tempdir();
    let f = tmp.path().join("Archive.zip");
    fs::write(&f, "x").unwrap();
    let upper = tmp.path().join("ARCHIVE.ZIP");
    assert_eq!(same_entity(&f, &upper), Some(true));
}

// R-325-1 / AC-321-4 (b): junction alias.
#[test]
fn same_entity_sees_through_a_junction_alias() {
    let tmp = tempdir();
    let real = tmp.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("x.txt"), "x").unwrap();
    let link = tmp.path().join("link");
    if !make_dir_link(&link, &real) {
        eprintln!("SKIP: cannot create a junction/symlink here");
        return;
    }
    assert_eq!(
        same_entity(&real.join("x.txt"), &link.join("x.txt")),
        Some(true)
    );
    remove_dir_link(&link);
}

// MUST-65 background: a path that does not exist cannot be identified -> `None`
// (callers on a destructive path treat `None` as "reject"; the ZIP-self check
// must therefore NOT call this for a non-existent final path, see
// `extract_plan_tests`).
#[test]
fn same_entity_is_none_when_a_path_does_not_exist() {
    let tmp = tempdir();
    let f = tmp.path().join("a.txt");
    fs::write(&f, "x").unwrap();
    assert_eq!(same_entity(&f, &tmp.path().join("missing.txt")), None);
}

// -------------------------------------------------------- is_strictly_within

#[test]
fn is_strictly_within_is_true_for_a_descendant() {
    let tmp = tempdir();
    let outer = tmp.path().join("outer");
    let inner = outer.join("a").join("b");
    fs::create_dir_all(&inner).unwrap();
    assert_eq!(is_strictly_within(&outer, &inner), Some(true));
}

#[test]
fn is_strictly_within_is_false_for_the_same_directory() {
    let tmp = tempdir();
    let d = tmp.path().join("d");
    fs::create_dir(&d).unwrap();
    assert_eq!(is_strictly_within(&d, &d), Some(false));
}

// Component-wise: `parent2` is not beneath `parent` (no string-prefix match).
#[test]
fn is_strictly_within_is_component_wise_not_a_string_prefix() {
    let tmp = tempdir();
    let parent = tmp.path().join("parent");
    let parent2 = tmp.path().join("parent2");
    fs::create_dir_all(&parent).unwrap();
    fs::create_dir_all(&parent2).unwrap();
    assert_eq!(is_strictly_within(&parent, &parent2), Some(false));
}

// R-325-1 / AC-321-4 (a): letter-case alias of the outer directory (Windows).
#[cfg(windows)]
#[test]
fn is_strictly_within_sees_through_letter_case_on_windows() {
    let tmp = tempdir();
    let outer = tmp.path().join("Docs");
    let inner = outer.join("x");
    fs::create_dir_all(&inner).unwrap();
    let outer_upper = tmp.path().join("DOCS");
    assert_eq!(is_strictly_within(&outer_upper, &inner), Some(true));
}

#[test]
fn is_strictly_within_is_none_when_a_path_cannot_be_canonicalized() {
    let tmp = tempdir();
    let outer = tmp.path().join("outer");
    fs::create_dir(&outer).unwrap();
    assert_eq!(is_strictly_within(&outer, &outer.join("missing")), None);
}

// ------------------------------------------------ normalize_allowing_missing
// MUST-69 contract (SPEC-0200-0001 §3.4.13-5).

// (c) fully existing path == canonicalize.
#[test]
fn nam_existing_path_equals_canonicalize() {
    let tmp = tempdir();
    let d = tmp.path().join("sub");
    fs::create_dir(&d).unwrap();
    assert_eq!(normalize_allowing_missing(&d), Some(canon(&d)));
}

// (c) longest existing prefix is canonicalized, the missing tail appended.
#[test]
fn nam_missing_tail_is_appended_to_the_canonical_existing_prefix() {
    let tmp = tempdir();
    let base = tmp.path().join("exists");
    fs::create_dir(&base).unwrap();
    let got = normalize_allowing_missing(&base.join("new").join("deeper.txt"));
    assert_eq!(got, Some(canon(&base).join("new").join("deeper.txt")));
}

// R-343-2 (2): nothing below the temp dir exists.
#[test]
fn nam_all_missing_below_an_existing_ancestor() {
    let tmp = tempdir();
    let got = normalize_allowing_missing(&tmp.path().join("a").join("b").join("c"));
    assert_eq!(got, Some(canon(tmp.path()).join("a").join("b").join("c")));
}

// (c) `..` in the missing remainder -> None (also R-343-2: `newdir\..\..\x`).
#[test]
fn nam_parent_dir_in_the_missing_remainder_is_none() {
    let tmp = tempdir();
    let p = tmp.path().join("nope").join("..").join("x");
    assert_eq!(normalize_allowing_missing(&p), None);
    let p2 = tmp.path().join("nope").join("..").join("..").join("x");
    assert_eq!(normalize_allowing_missing(&p2), None);
}

// (b) components(), not string concatenation: forward slashes are separators.
#[test]
fn nam_forward_slash_separated_input_is_split_into_components() {
    let tmp = tempdir();
    let base = tmp.path().join("exists");
    fs::create_dir(&base).unwrap();
    let with_slashes = PathBuf::from(format!("{}/new/x.txt", base.display()));
    assert_eq!(
        normalize_allowing_missing(&with_slashes),
        Some(canon(&base).join("new").join("x.txt"))
    );
}

// A verbatim (`\\?\`) input is handled; the result agrees with canonicalize's
// own form (callers compare results of the same function only, §3.4.8).
#[cfg(windows)]
#[test]
fn nam_accepts_a_verbatim_prefixed_input() {
    let tmp = tempdir();
    let verbatim = canon(tmp.path());
    assert!(
        verbatim.to_string_lossy().starts_with(r"\\?\"),
        "canonicalize returns verbatim paths on Windows"
    );
    let got = normalize_allowing_missing(&verbatim.join("new"));
    assert_eq!(got, Some(verbatim.join("new")));
}

// (a) relative input -> None (the caller joins `base_dir` first).
#[test]
fn nam_relative_path_is_none() {
    assert_eq!(normalize_allowing_missing(Path::new("a/b")), None);
    assert_eq!(normalize_allowing_missing(Path::new("out.zip")), None);
    assert_eq!(normalize_allowing_missing(Path::new("")), None);
}

// (a) Windows drive-relative (`C:x`) and root-relative (`\x`) are not absolute.
#[cfg(windows)]
#[test]
fn nam_drive_relative_and_root_relative_are_none() {
    assert_eq!(normalize_allowing_missing(Path::new("C:x.zip")), None);
    assert_eq!(normalize_allowing_missing(Path::new(r"\x.zip")), None);
}

// (d) an existing but dangling link cannot be canonicalized -> None, whether it
// is the whole path or an ancestor of the missing tail.
#[test]
fn nam_dangling_link_is_none() {
    let tmp = tempdir();
    let target = tmp.path().join("gone");
    fs::create_dir(&target).unwrap();
    let link = tmp.path().join("dangling");
    if !make_dir_link(&link, &target) {
        eprintln!("SKIP: cannot create a junction/symlink here");
        return;
    }
    fs::remove_dir(&target).unwrap();
    assert_eq!(
        normalize_allowing_missing(&link),
        None,
        "dangling link itself"
    );
    assert_eq!(
        normalize_allowing_missing(&link.join("x.txt")),
        None,
        "tail below a dangling link"
    );
    remove_dir_link(&link);
}

// (c) a healthy link in the existing prefix is resolved (junction alias ->
// the real directory), so two spellings normalize to the same path (R-325-1).
#[test]
fn nam_resolves_a_link_in_the_existing_prefix() {
    let tmp = tempdir();
    let real = tmp.path().join("real");
    fs::create_dir(&real).unwrap();
    let link = tmp.path().join("link");
    if !make_dir_link(&link, &real) {
        eprintln!("SKIP: cannot create a junction/symlink here");
        return;
    }
    assert_eq!(
        normalize_allowing_missing(&link.join("out.zip")),
        normalize_allowing_missing(&real.join("out.zip"))
    );
    assert_eq!(
        normalize_allowing_missing(&link.join("out.zip")),
        Some(canon(&real).join("out.zip"))
    );
    remove_dir_link(&link);
}

// Letter-case alias of an existing prefix normalizes identically (R-325-1).
#[cfg(windows)]
#[test]
fn nam_letter_case_of_the_existing_prefix_is_normalized() {
    let tmp = tempdir();
    let base = tmp.path().join("Work");
    fs::create_dir(&base).unwrap();
    let a = normalize_allowing_missing(&base.join("out.zip"));
    let b = normalize_allowing_missing(&tmp.path().join("WORK").join("out.zip"));
    assert!(a.is_some());
    assert_eq!(a, b);
}

// Drive root exists (`C:\`): result is the canonical root plus the tail.
#[cfg(windows)]
#[test]
fn nam_drive_root_with_a_missing_tail() {
    let root = Path::new(r"C:\");
    if !root.exists() {
        eprintln!("SKIP: no C: drive");
        return;
    }
    let got = normalize_allowing_missing(Path::new(r"C:\rfz_definitely_missing_dir_0f3a\a.zip"));
    assert_eq!(
        got,
        Some(
            canon(root)
                .join("rfz_definitely_missing_dir_0f3a")
                .join("a.zip")
        )
    );
    assert_eq!(normalize_allowing_missing(root), Some(canon(root)));
}

// A drive that does not exist has no existing prefix -> None.
#[cfg(windows)]
#[test]
fn nam_nonexistent_drive_is_none() {
    let missing = ('D'..='Z')
        .rev()
        .map(|c| format!("{c}:\\"))
        .find(|d| !Path::new(d).exists());
    let Some(drive) = missing else {
        eprintln!("SKIP: every drive letter exists");
        return;
    };
    assert_eq!(
        normalize_allowing_missing(&Path::new(&drive).join("a").join("b.zip")),
        None
    );
}
