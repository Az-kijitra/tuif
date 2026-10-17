//! TDD (unit-test-writer, 2026-10-07): the two PURE helpers behind the ZIP absolute-path check.
//! Specs: SPEC-0300-0011 第9版 T-14 (`/` -> `\` on the output / destination INPUT before resolving);
//! SPEC-0300-0014 第5版 R-322-1 / R-341-1 (relative input joins `base_dir`; drive-relative and
//! root-relative stay `invalid_path`); SPEC-0200-0001 §3.12.5 (`normalize_input_separators`,
//! `join_destination` = the pure first half of `resolve_destination`, behaviour unchanged).
//!

use super::error::ArchiveErrorCode as Code;
use super::extract_plan::{join_destination, resolve_destination};
use super::path_rules::normalize_input_separators;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- normalize_input_separators (T-14)

#[test]
fn every_forward_slash_becomes_a_backslash() {
    assert_eq!(normalize_input_separators("D:/backup/out.zip"), r"D:\backup\out.zip");
    assert_eq!(normalize_input_separators("sub/out.zip"), r"sub\out.zip");
    assert_eq!(normalize_input_separators("//server/share/x"), r"\\server\share\x");
    assert_eq!(normalize_input_separators("a/b/c/"), r"a\b\c\");
    assert_eq!(normalize_input_separators("/"), r"\");
}

#[test]
fn nothing_else_is_touched() {
    for same in ["", "out", r"D:\backup\out.zip", r"\\server\share", "C:", r"C:src", "  spaced name  ", "日本語\\名前", "a b.zip"] {
        assert_eq!(normalize_input_separators(same), same, "{same:?}");
    }
    // no trimming, no collapsing of repeated separators, no `.zip` handling
    assert_eq!(normalize_input_separators("a//b"), r"a\\b");
    assert_eq!(normalize_input_separators(" x/ "), r" x\ ");
}

// A `\\?\` input with a `/` gets the `/` replaced too (names cannot contain `/`, so no real harm; §3.12.5).
#[test]
fn a_slash_inside_a_verbatim_input_is_replaced_as_well() {
    assert_eq!(normalize_input_separators(r"\\?\C:\a/b"), r"\\?\C:\a\b");
}

#[test]
fn it_is_idempotent() {
    let once = normalize_input_separators("D:/a/b\\c/d");
    assert_eq!(normalize_input_separators(&once), once);
}

// ---------------------------------------------------------------- join_destination (pure, behaviour = old resolve_destination 1st half)

fn base() -> PathBuf {
    PathBuf::from(r"C:\zz-no-such-base-folder\cur")
}

fn code(result: Result<PathBuf, super::error::ArchiveError>) -> Code {
    match result {
        Err(e) => e.code,
        Ok(p) => panic!("expected an error, got {}", p.display()),
    }
}

#[test]
fn an_empty_destination_is_invalid_path() {
    assert_eq!(code(join_destination(Path::new(""), &base())), Code::InvalidPath);
}

#[test]
fn an_absolute_destination_is_returned_as_is_without_touching_the_file_system() {
    // the paths do not exist: a function that stat()s would fail here
    for abs in [r"C:\zz-nowhere\out", r"D:\x", r"\\server\share\x", r"\\?\C:\zz-nowhere\y"] {
        assert_eq!(join_destination(Path::new(abs), &base()).unwrap(), PathBuf::from(abs), "{abs}");
    }
}

#[test]
fn a_relative_destination_joins_the_base_dir() {
    assert_eq!(join_destination(Path::new("sub"), &base()).unwrap(), base().join("sub"));
    assert_eq!(join_destination(Path::new(r"sub\x"), &base()).unwrap(), base().join(r"sub\x"));
    assert_eq!(join_destination(Path::new(r".\x"), &base()).unwrap(), base().join(r".\x"), "no normalisation here");
}

#[test]
fn drive_relative_and_root_relative_destinations_are_invalid_path() {
    for bad in ["C:out", "C:", r"\out", r"\", "D:x"] {
        assert_eq!(code(join_destination(Path::new(bad), &base())), Code::InvalidPath, "{bad}");
    }
}

#[test]
fn a_relative_destination_with_a_non_absolute_base_dir_is_invalid_path() {
    for bad_base in ["", "rel", r"C:rel", r"\root"] {
        assert_eq!(code(join_destination(Path::new("sub"), Path::new(bad_base))), Code::InvalidPath, "base {bad_base:?}");
    }
}

// An absolute destination does not need the base_dir at all (the old code never looked at it either).
#[test]
fn an_absolute_destination_ignores_the_base_dir() {
    assert_eq!(join_destination(Path::new(r"C:\zz\x"), Path::new("")).unwrap(), PathBuf::from(r"C:\zz\x"));
}

// Device-style destinations are "absolute" for Rust (so they are NOT joined); T-13 then rejects them later.
#[cfg(windows)]
#[test]
fn rust_absolute_device_style_inputs_are_returned_unjoined_for_the_resolved_check_to_reject() {
    for p in [r"\\.\PhysicalDrive0", r"\\.\pipe\x"] {
        assert_eq!(join_destination(Path::new(p), &base()).unwrap(), PathBuf::from(p), "{p}");
        assert!(!crate::shared::abs_path::is_absolute_path_buf(Path::new(p)), "T-13 rejects the resolved {p}");
    }
}

// resolve_destination keeps its behaviour: it calls join_destination first, then the file-system part.
#[test]
fn resolve_destination_still_rejects_the_same_inputs_and_still_normalizes() {
    let dir = super::test_support::tempdir();
    let base_dir = dir.path().to_path_buf();
    assert_eq!(resolve_destination(Path::new(""), &base_dir).unwrap_err().code, Code::InvalidPath);
    assert_eq!(resolve_destination(Path::new("C:out"), &base_dir).unwrap_err().code, Code::InvalidPath);
    assert_eq!(resolve_destination(Path::new(r"\out"), &base_dir).unwrap_err().code, Code::InvalidPath);
    assert_eq!(resolve_destination(Path::new("sub"), Path::new("rel")).unwrap_err().code, Code::InvalidPath);
    let resolved = resolve_destination(Path::new("sub"), &base_dir).expect("relative joins the base");
    assert!(resolved.ends_with("sub"), "{}", resolved.display());
    let file = dir.path().join("f.txt");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(resolve_destination(&file, &base_dir).unwrap_err().code, Code::DestinationNotDir);
}
