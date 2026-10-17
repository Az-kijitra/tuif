//! TDD (unit-test-writer, 2026-10-07): the inner layer of `fs_list_dir`.
//! Specs: SPEC-0300-0011 第9版 R-89 / R-89-1 #1 / R-89-2 / R-85-4; SPEC-0300-0009 第14版 R-70-1 / AC-643;
//! SPEC-0300-0016 AC-751 (absolute paths: unchanged); SPEC-0200-0001 §3.12.3 #1 / §3.12.4.
//!
//! `list_dir_checked(path, show_hidden, sort_key, sort_order)` = `abs(path)` -> `query::fs_list_dir`.
//! The process working directory is NEVER changed (other tests run in parallel); a relative path that
//! might resolve against it (`.`, `src`, a random name) must still be refused.


use std::io;

use super::commands::{list_dir_checked, FsListDirErrorDto};

fn rand_name() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("zz-abs-path-{nanos}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

fn list(path: &str) -> Result<usize, FsListDirErrorDto> {
    list_dir_checked(path, true, "name", "asc").map(|entries| entries.len())
}

// AC-643: every non-absolute form is `invalid_path`.
#[test]
fn ac643_relative_drive_relative_root_relative_and_empty_paths_are_invalid_path() {
    let random = rand_name();
    for bad in ["C:", "C:src", "src", ".", "..", r"\foo", "/foo", "", r".\src", "C:/foo", r"\\server", r"\\.\PhysicalDrive0", random.as_str()] {
        let err = list(bad).expect_err(&format!("{bad:?} must be refused"));
        assert_eq!(err.code, "invalid_path", "{bad:?}: {err:?}");
        assert!(!err.message.is_empty(), "{bad:?}: the message is for display and must not be empty");
    }
}

// R-89 (nothing touched): refusing a random relative name leaves no trace in the working directory.
#[test]
fn ac643_a_refused_random_relative_name_creates_nothing_in_the_working_directory() {
    let random = rand_name();
    let _ = list(&random);
    assert!(!std::path::Path::new(&random).exists());
}

// AC-643 "絶対パスは従来どおり": a real folder lists, a missing one is not_found, a file is not_a_directory.
#[test]
fn ac643_an_absolute_folder_still_lists_its_entries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    let entries = list_dir_checked(&dir.path().to_string_lossy(), true, "name", "asc").expect("absolute path lists");
    let mut names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["a.txt", "d"]);
}

#[test]
fn ac643_an_absolute_missing_path_is_still_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("does-not-exist-9271");
    let err = list(&missing.to_string_lossy()).expect_err("missing");
    assert_eq!(err.code, "not_found");
}

#[cfg(windows)]
#[test]
fn ac643_an_absolute_file_is_still_not_a_directory_ac751() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.txt");
    std::fs::write(&file, "x").unwrap();
    let err = list(&file.to_string_lossy()).expect_err("a file");
    assert_eq!(err.code, "not_a_directory");
}

// The extended and UNC absolute forms pass the check (a missing UNC share then fails by itself, not as invalid_path).
#[cfg(windows)]
#[test]
fn ac643_absolute_extended_form_is_not_refused_as_invalid_path() {
    let dir = tempfile::tempdir().unwrap();
    let verbatim = format!(r"\\?\{}", dir.path().display());
    assert!(list(&verbatim).is_ok());
}

// §3.12.4: the constructor, and `From<io::Error>` is NOT changed (no ErrorKind maps to invalid_path).
#[test]
fn the_invalid_path_constructor_gives_code_invalid_path_and_a_message() {
    let dto = FsListDirErrorDto::invalid_path("src");
    assert_eq!(dto.code, "invalid_path");
    assert!(!dto.message.is_empty());
}

#[test]
fn io_errors_never_map_to_invalid_path() {
    for kind in [io::ErrorKind::InvalidInput, io::ErrorKind::InvalidData, io::ErrorKind::Other, io::ErrorKind::NotFound] {
        let dto: FsListDirErrorDto = io::Error::new(kind, "x").into();
        assert_ne!(dto.code, "invalid_path", "{kind:?}");
    }
}
