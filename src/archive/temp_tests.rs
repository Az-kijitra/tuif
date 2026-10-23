//! TDD (unit-test-writer, 2026-09-27): `archive::temp`.
//!
//! Specs: SPEC-0300-0014 R-328 / R-328-1 / R-328-2 (temp file removed on
//! failure / cancel), R-328-4 + R-348-1 (name independent of the final name,
//! fixed length), R-348; SPEC-0200-0001 §3.4.13-6 (`.rfz-<pid>-<counter>.tmp`,
//! `create_new` retry limit 100), SHOULD-87 (`create_new` refuses a link, RAII).

use super::temp::*;
use super::test_support::*;
use std::io::{Read, Write};
use std::path::PathBuf;

fn is_temp_shaped(name: &str) -> bool {
    name.starts_with(".rfz-") && name.ends_with(".tmp")
}

// R-328-4 / R-348-1: `.rfz-<pid>-<counter>.tmp`.
#[test]
fn r328_4_temp_file_name_has_the_fixed_shape() {
    assert_eq!(temp_file_name(1234, 7), ".rfz-1234-7.tmp");
    assert_eq!(temp_file_name(0, 0), ".rfz-0-0.tmp");
}

// R-328-4: the name has a bounded length regardless of anything else (there is
// no final-name parameter at all, so a long final name cannot lengthen it).
#[test]
fn r328_4_temp_file_name_length_is_bounded() {
    let longest = temp_file_name(u32::MAX, u64::MAX);
    assert!(longest.len() <= 64, "{longest}");
    assert!(is_temp_shaped(&longest));
    assert_ne!(temp_file_name(1, 1), temp_file_name(1, 2));
    assert_ne!(temp_file_name(1, 1), temp_file_name(2, 1));
}

#[test]
fn create_in_makes_an_empty_temp_file_with_the_fixed_shape() {
    let tmp = tempdir();
    let t = TempFile::create_in(tmp.path()).expect("create");
    let name = t.path().file_name().unwrap().to_string_lossy().into_owned();
    assert!(is_temp_shaped(&name), "{name}");
    assert_eq!(
        t.path().parent().unwrap(),
        tmp.path(),
        "created in the requested directory"
    );
    assert_eq!(std::fs::metadata(t.path()).unwrap().len(), 0);
}

#[test]
fn create_in_yields_distinct_paths() {
    let tmp = tempdir();
    let a = TempFile::create_in(tmp.path()).unwrap();
    let b = TempFile::create_in(tmp.path()).unwrap();
    assert_ne!(a.path(), b.path());
}

#[test]
fn create_in_fails_for_a_missing_directory() {
    let tmp = tempdir();
    assert!(TempFile::create_in(&tmp.path().join("no-such-dir")).is_err());
}

// RAII (R-328-1 / R-328-2 / R-348): dropping deletes the file.
#[test]
fn drop_deletes_the_temp_file() {
    let tmp = tempdir();
    let path = {
        let mut t = TempFile::create_in(tmp.path()).unwrap();
        t.file().write_all(b"partial data").unwrap();
        t.path().to_path_buf()
    };
    assert!(
        !path.exists(),
        "the guard must delete the temp file on drop"
    );
    assert!(tree_names(tmp.path()).is_empty());
}

// SHOULD-87: cleanup also happens when the operation panics.
#[test]
fn drop_deletes_the_temp_file_even_when_the_operation_panics() {
    let tmp = tempdir();
    let dir = tmp.path().to_path_buf();
    let result = std::panic::catch_unwind(move || {
        let _t = TempFile::create_in(&dir).unwrap();
        panic!("simulated failure while writing");
    });
    assert!(result.is_err());
    assert!(
        tree_names(tmp.path()).is_empty(),
        "no temp file may survive a panic"
    );
}

// `into_path` disarms the guard: the caller renames the file to its final name.
#[test]
fn into_path_keeps_the_file_and_its_content() {
    let tmp = tempdir();
    let mut t = TempFile::create_in(tmp.path()).unwrap();
    t.file().write_all(b"finished").unwrap();
    let path: PathBuf = t.into_path();
    assert!(path.exists());
    let mut content = String::new();
    std::fs::File::open(&path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert_eq!(content, "finished");
    // The kept file can be renamed to a final name (Windows: handle is closed).
    let final_path = tmp.path().join("final.bin");
    std::fs::rename(&path, &final_path).expect("rename after into_path");
    assert_eq!(read_string(&final_path), "finished");
}

// §3.4.13-6: `create_new` — an existing file is never reused or truncated; the
// generator is asked again.
#[test]
fn create_new_retries_with_the_next_name_and_never_touches_an_existing_file() {
    let tmp = tempdir();
    write_file(tmp.path(), ".taken.tmp", b"keep me");
    let mut names = vec![".taken.tmp", ".taken.tmp", ".fresh.tmp"].into_iter();
    let mut calls = 0;
    let mut generator = || {
        calls += 1;
        names
            .next()
            .expect("more attempts than expected")
            .to_string()
    };
    let t = TempFile::create_in_with(tmp.path(), &mut generator).expect("third attempt succeeds");
    assert_eq!(t.path().file_name().unwrap(), ".fresh.tmp");
    assert_eq!(calls, 3);
    assert_eq!(read_string(&tmp.path().join(".taken.tmp")), "keep me");
}

// §3.4.13-6: retry limit 100 -> `io::Error`.
#[test]
fn create_new_gives_up_after_exactly_100_attempts() {
    assert_eq!(MAX_CREATE_NEW_ATTEMPTS, 100);
    let tmp = tempdir();
    write_file(tmp.path(), ".taken.tmp", b"keep me");
    let mut calls = 0u32;
    let mut generator = || {
        calls += 1;
        ".taken.tmp".to_string()
    };
    let result = TempFile::create_in_with(tmp.path(), &mut generator);
    assert!(result.is_err(), "must fail after the limit");
    assert_eq!(calls, 100, "exactly MAX_CREATE_NEW_ATTEMPTS attempts");
    assert_eq!(read_string(&tmp.path().join(".taken.tmp")), "keep me");
}

// SHOULD-87: a link (with no target) sitting on the chosen name is never
// written through; the creation either retries or fails, and the link's target
// is NOT created.
#[test]
fn should87_create_new_does_not_write_through_a_dangling_directory_link() {
    let tmp = tempdir();
    let link = tmp.path().join(".rfz-link.tmp");
    let target = tmp.path().join("scratch-target");
    if !make_dangling_dir_link(&link, &target) {
        skip("cannot create a dangling junction/symlink here");
        return;
    }
    let mut names = vec![".rfz-link.tmp".to_string(), ".rfz-ok.tmp".to_string()].into_iter();
    let mut generator = || names.next().unwrap_or_else(|| ".rfz-last.tmp".to_string());
    let outcome = TempFile::create_in_with(tmp.path(), &mut generator);
    assert!(
        !target.exists(),
        "the link target must not have been created through the link"
    );
    drop(outcome);
    remove_dir_link(&link);
}

#[test]
fn should87_create_new_does_not_write_through_a_dangling_file_symlink() {
    let tmp = tempdir();
    let link = tmp.path().join(".rfz-link.tmp");
    let target = tmp.path().join("victim.txt");
    if !make_file_symlink(&link, &target) {
        skip("file symlinks need the symlink privilege on Windows");
        return;
    }
    let mut names = vec![".rfz-link.tmp".to_string(), ".rfz-ok.tmp".to_string()].into_iter();
    let mut generator = || names.next().unwrap_or_else(|| ".rfz-last.tmp".to_string());
    let outcome = TempFile::create_in_with(tmp.path(), &mut generator);
    assert!(
        !target.exists(),
        "create_new must refuse a dangling symlink instead of creating its target"
    );
    drop(outcome);
}
