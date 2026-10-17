//! Tests for the debug-only configuration-folder override
//! (`REFLEXFILES_CONFIG_DIR`): SPEC-0400-0001 第2版 R-06~R-08 / AC-06~AC-08,
//! SPEC-0300-0017 §18.2 / AC-947, SPEC-0200-0001 §3.8.10-1 as overridden by
//! §3.8.17 MUST-108 (`var_os`; only "variable absent" means the default; set but
//! empty / blank / non-Unicode / relative / `..` / a file => error), MUST-109
//! (one stderr line, debug only) and SHOULD-155 (the validation function is
//! compiled and tested in EVERY build; tests never mutate the process
//! environment -- raw values are passed straight to the pure function).
//!
//! The error TYPE's variants are not part of the contract tested here: only
//! `is_err()` and the `io::ErrorKind::InvalidInput` conversion are.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

use super::paths::{override_notice_line, resolve_config_dir_override};

const PATHS_SRC: &str = include_str!("paths.rs");
const THIS_SRC: &str = include_str!("paths_tests.rs");

fn resolve(p: &Path) -> Result<Option<PathBuf>, super::paths::ConfigDirOverrideError> {
    resolve_config_dir_override(Some(p.as_os_str()))
}

fn dir_is_empty(p: &Path) -> bool {
    fs::read_dir(p).unwrap().next().is_none()
}

// ---------------------------------------------------------------- default

#[test]
fn variable_absent_means_use_the_default_folder() {
    assert_eq!(resolve_config_dir_override(None).unwrap(), None);
}

// ------------------------------------------------- rejected (MUST-108)

#[test]
fn set_but_empty_is_an_error_not_the_default() {
    assert!(resolve_config_dir_override(Some(OsStr::new(""))).is_err());
}

#[test]
fn whitespace_only_is_an_error() {
    for raw in [" ", "   ", "\t", " \n ", "\u{3000}"] {
        assert!(
            resolve_config_dir_override(Some(OsStr::new(raw))).is_err(),
            "{raw:?} must be rejected"
        );
    }
}

#[cfg(windows)]
fn non_unicode_under(dir: &Path) -> OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let mut wide: Vec<u16> = dir.as_os_str().encode_wide().collect();
    wide.push(b'\\' as u16);
    wide.push(0xD800); // an unpaired surrogate: not valid Unicode
    wide.push(b'x' as u16);
    OsString::from_wide(&wide)
}

#[cfg(unix)]
fn non_unicode_under(dir: &Path) -> OsString {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let mut bytes = dir.as_os_str().as_bytes().to_vec();
    bytes.extend_from_slice(b"/");
    bytes.push(0xFF);
    OsString::from_vec(bytes)
}

#[test]
fn non_unicode_is_an_error_even_when_it_looks_absolute_and_nothing_is_created() {
    let tmp = tempdir().unwrap();
    let raw = non_unicode_under(tmp.path());
    assert!(raw.to_str().is_none(), "fixture must be non-Unicode");
    assert!(resolve_config_dir_override(Some(raw.as_os_str())).is_err());
    assert!(dir_is_empty(tmp.path()), "a rejected value must not create anything");
}

#[test]
fn relative_paths_are_rejected_and_not_created_in_the_current_directory() {
    let marker = "reflexfiles_rel_override_must_not_exist";
    let cases = [marker.to_string(), format!("{marker}/sub"), format!("./{marker}"), ".".to_string()];
    for raw in cases {
        assert!(
            resolve_config_dir_override(Some(OsStr::new(&raw))).is_err(),
            "{raw:?} is relative and must be rejected"
        );
    }
    assert!(!Path::new(marker).exists(), "relative value created a directory in the CWD");
}

#[cfg(windows)]
#[test]
fn windows_drive_relative_and_rooted_without_drive_are_rejected() {
    // `C:config` (relative to the drive's current directory) and `\config`
    // (rooted, no drive) are not absolute paths.
    for raw in ["C:reflexfiles_rel_override_must_not_exist", "\\reflexfiles_rel_override_must_not_exist"] {
        assert!(resolve_config_dir_override(Some(OsStr::new(raw))).is_err(), "{raw:?}");
    }
}

#[test]
fn parent_directory_components_are_rejected_and_nothing_is_created() {
    let tmp = tempdir().unwrap();
    let with_dotdot = tmp.path().join("a").join("..").join("b");
    assert!(resolve(&with_dotdot).is_err());
    let trailing = tmp.path().join("..");
    assert!(resolve(&trailing).is_err());
    assert!(dir_is_empty(tmp.path()), "validation must happen before any directory is created");
}

#[test]
fn a_path_that_points_to_an_existing_file_is_rejected() {
    let tmp = tempdir().unwrap();
    let file = tmp.path().join("not-a-dir.txt");
    fs::write(&file, b"keep me").unwrap();
    assert!(resolve(&file).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"keep me", "the file must be left untouched");
}

#[test]
fn rejection_converts_to_io_error_invalid_input() {
    // The caller (`app_config_dir`) maps the failure to `io::Error(InvalidInput)`;
    // it must never silently fall back to the default folder (MUST-108).
    let err = resolve_config_dir_override(Some(OsStr::new(""))).unwrap_err();
    let io_err: io::Error = err.into();
    assert_eq!(io_err.kind(), io::ErrorKind::InvalidInput);
}

// ----------------------------------------------------------- accepted

#[test]
fn a_missing_absolute_directory_is_created_and_returned_unchanged() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("new-config");
    assert!(!target.exists());
    let got = resolve(&target).unwrap();
    assert_eq!(got, Some(target.clone()));
    assert!(target.is_dir());
}

#[test]
fn missing_intermediate_directories_are_created_too() {
    let tmp = tempdir().unwrap();
    let target = tmp.path().join("a").join("b").join("c");
    assert_eq!(resolve(&target).unwrap(), Some(target.clone()));
    assert!(target.is_dir());
}

#[test]
fn an_existing_directory_is_accepted_and_its_contents_are_untouched() {
    let tmp = tempdir().unwrap();
    let keep = tmp.path().join("ui-state.toml");
    fs::write(&keep, b"x = 1").unwrap();
    assert_eq!(resolve(tmp.path()).unwrap(), Some(tmp.path().to_path_buf()));
    assert_eq!(fs::read(&keep).unwrap(), b"x = 1");
}

#[cfg(windows)]
#[test]
fn a_verbatim_prefix_path_is_allowed_not_special_cased() {
    // §3.8.10-1 (6): UNC / `\\?\` prefixes are tolerated.
    let tmp = tempdir().unwrap();
    let verbatim = PathBuf::from(format!(r"\\?\{}", tmp.path().display())).join("verbatim-new");
    let got = resolve(&verbatim).unwrap();
    assert_eq!(got, Some(verbatim.clone()));
    assert!(tmp.path().join("verbatim-new").is_dir());
}

// ------------------------------------------- stderr line (MUST-109)

#[test]
fn notice_line_is_exactly_one_line_with_the_documented_prefix() {
    let tmp = tempdir().unwrap();
    let line = override_notice_line(tmp.path());
    assert_eq!(line, format!("[config] override dir: {}", tmp.path().display()));
    assert!(!line.contains('\n'));
}

/// Every `eprintln!` in non-test source must sit in a function that is gated by
/// `debug_assertions` (attribute above the fn, or a `cfg!` / `#[cfg]` inside).
fn ungated_eprintln_lines(src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut bad = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("//") || !t.contains("eprintln!") {
            continue;
        }
        let header = (0..=i).rev().find(|&j| {
            let l = lines[j].trim_start();
            l.starts_with("fn ") || l.starts_with("pub fn ") || l.starts_with("pub(crate) fn ")
        });
        let Some(fi) = header else {
            bad.push(format!("line {}: eprintln! outside any fn", i + 1));
            continue;
        };
        let mut start = fi;
        while start > 0 {
            let p = lines[start - 1].trim_start();
            if p.starts_with("#[") || p.starts_with("//") {
                start -= 1;
            } else {
                break;
            }
        }
        if !lines[start..=i].join("\n").contains("debug_assertions") {
            bad.push(format!("line {}: eprintln! not gated by debug_assertions", i + 1));
        }
    }
    bad
}

#[test]
fn gating_checker_self_check() {
    let gated = "#[cfg(debug_assertions)]\nfn a() {\n    eprintln!(\"x\");\n}\n";
    let gated_inline = "fn b() {\n    if cfg!(debug_assertions) {\n        eprintln!(\"x\");\n    }\n}\n";
    let ungated = "fn c() {\n    eprintln!(\"x\");\n}\n";
    assert!(ungated_eprintln_lines(gated).is_empty());
    assert!(ungated_eprintln_lines(gated_inline).is_empty());
    assert_eq!(ungated_eprintln_lines(ungated).len(), 1);
}

#[test]
fn the_stderr_notice_exists_and_is_debug_only() {
    assert!(
        PATHS_SRC.contains("eprintln!"),
        "MUST-109: the override must announce itself on stderr (debug builds)"
    );
    assert_eq!(ungated_eprintln_lines(PATHS_SRC), Vec::<String>::new());
}

// ----------------------------------- source rules (MUST-108 / SHOULD-155)

fn code_only(src: &str) -> String {
    src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_variable_is_read_with_var_os_never_with_the_lossy_var() {
    let code = code_only(PATHS_SRC);
    assert!(code.contains("REFLEXFILES_CONFIG_DIR"), "the variable name must be defined in paths.rs");
    assert!(code.contains("var_os"), "MUST-108: use std::env::var_os (non-Unicode must not become 'unset')");
    assert!(!code.contains("env::var("), "MUST-108: std::env::var(..) hides non-Unicode values");
}

#[test]
fn the_validation_function_is_not_gated_by_debug_assertions() {
    let lines: Vec<&str> = PATHS_SRC.lines().collect();
    let idx = lines
        .iter()
        .position(|l| l.contains("pub fn resolve_config_dir_override"))
        .expect("resolve_config_dir_override must exist");
    let mut i = idx;
    while i > 0 {
        let p = lines[i - 1].trim_start();
        if p.starts_with("#[") || p.starts_with("///") {
            // Only attributes matter; doc comments may legitimately mention the cfg.
            assert!(
                !(p.starts_with("#[") && p.contains("debug_assertions")),
                "SHOULD-155: the validator must be compiled in every build so cargo test covers it"
            );
            i -= 1;
        } else {
            break;
        }
    }
}

#[test]
fn tests_and_paths_never_mutate_the_process_environment() {
    let needle = ["set", "_var"].concat();
    assert!(!THIS_SRC.contains(&needle), "SHOULD-155: pass raw values to the pure function");
    assert!(!PATHS_SRC.contains(&needle));
}
