//! `navigation.toml` load/save (SPEC-0300-0013 R-239~R-246, R-205-2, R-253-2;
//! SPEC-0200-0001 §3.2.2, MUST-48). Planned API (not yet implemented, TDD):
//!
//! ```ignore
//! pub struct NavigationLoadResult { pub data: NavigationData, pub recovered_from_corruption: bool }
//! pub fn load(path: &Path) -> io::Result<NavigationLoadResult>
//! pub fn save(path: &Path, data: &NavigationData) -> io::Result<()>
//! ```
//! `load`: `fs::read` (NotFound => empty, no backup); non-UTF-8 OR invalid TOML
//! => rename to `<path>.toml.bak` (overwriting), return empty +
//! `recovered_from_corruption = true`; rename failure or other I/O error => Err;
//! valid data is truncated to 50/20/20 (no dedupe on the Rust side).

use std::fs;
use std::io;
use std::path::Path;

use crate::shared::schema::navigation::NavigationData;

const MAX_VISIT_HISTORY: usize = 50;
const MAX_JUMP_LIST: usize = 20;
const MAX_SEARCH_HISTORY: usize = 20;

pub struct NavigationLoadResult {
    pub data: NavigationData,
    /// R-242: true when an unparseable file was moved to `.bak`.
    pub recovered_from_corruption: bool,
}

pub fn load(path: &Path) -> io::Result<NavigationLoadResult> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(NavigationLoadResult {
                data: NavigationData::default(),
                recovered_from_corruption: false,
            });
        }
        Err(e) => return Err(e),
    };
    let parsed = String::from_utf8(bytes)
        .ok()
        .and_then(|s| toml::from_str::<NavigationData>(&s).ok());
    match parsed {
        Some(mut data) => {
            data.visit_history.truncate(MAX_VISIT_HISTORY);
            data.jump_list.truncate(MAX_JUMP_LIST);
            data.search_history.truncate(MAX_SEARCH_HISTORY);
            Ok(NavigationLoadResult { data, recovered_from_corruption: false })
        }
        None => {
            let backup_path = path.with_extension("toml.bak");
            fs::rename(path, &backup_path)?;
            Ok(NavigationLoadResult {
                data: NavigationData::default(),
                recovered_from_corruption: true,
            })
        }
    }
}

pub fn save(path: &Path, data: &NavigationData) -> io::Result<()> {
    let serialized = toml::to_string_pretty(data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::config::atomic::atomic_write(path, &serialized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::schema::navigation::NavigationData;
    use std::fs;

    fn strs(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{prefix}{i}")).collect()
    }

    // R-242-1 / AC-239: missing file is normal: empty data, no backup, no notice.
    #[test]
    fn missing_file_returns_defaults_without_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");

        let r = load(&path).expect("load");

        assert_eq!(r.data, NavigationData::default());
        assert!(!r.recovered_from_corruption);
        assert!(!dir.path().join("navigation.toml.bak").exists());
    }

    // R-245 / AC-241: round trip; an empty list saved stays empty.
    #[test]
    fn save_then_load_round_trips_three_lists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        let data = NavigationData {
            visit_history: vec!["C:\\a".into(), "C:\\b".into()],
            jump_list: vec!["C:\\j".into()],
            search_history: vec!["q1".into(), "q2".into()],
        };
        save(&path, &data).expect("save");
        let r = load(&path).expect("load");
        assert_eq!(r.data, data);
        assert!(!r.recovered_from_corruption);
    }

    #[test]
    fn saving_empty_lists_is_restored_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        save(&path, &NavigationData::default()).expect("save");
        let r = load(&path).expect("load");
        assert_eq!(r.data, NavigationData::default());
    }

    // R-239/R-241-1/AC-235/AC-237: one file, no leftover tmp.
    #[test]
    fn save_produces_a_single_file_and_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        save(&path, &NavigationData::default()).expect("save");
        let count = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 1);
        assert!(!dir.path().join("navigation.toml.tmp").exists());
    }

    // R-242 / AC-238: invalid TOML -> backed up, empty, recovered flag.
    #[test]
    fn invalid_toml_is_backed_up_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        let bad = "this is = = not toml [[[";
        fs::write(&path, bad).unwrap();

        let r = load(&path).expect("load");

        assert_eq!(r.data, NavigationData::default());
        assert!(r.recovered_from_corruption);
        assert_eq!(fs::read_to_string(dir.path().join("navigation.toml.bak")).unwrap(), bad);
        assert!(!path.exists(), "corrupt original must no longer sit at the load path");
    }

    // MUST-48(a): invalid UTF-8 is a parse failure too, not a bare Err.
    #[test]
    fn non_utf8_bytes_are_backed_up_like_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        let bytes: Vec<u8> = vec![0xff, 0xfe, 0x00, 0xc3, 0x28, 0xa0, 0xa1];
        fs::write(&path, &bytes).unwrap();

        let r = load(&path).expect("non-UTF-8 must be recovered, not Err");

        assert_eq!(r.data, NavigationData::default());
        assert!(r.recovered_from_corruption);
        assert_eq!(fs::read(dir.path().join("navigation.toml.bak")).unwrap(), bytes);
    }

    // AC-238 (last clause): a later save does not lose the backed-up file.
    #[test]
    fn backup_survives_a_subsequent_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        fs::write(&path, "garbage [[[").unwrap();
        load(&path).expect("load");
        save(&path, &NavigationData::default()).expect("save");
        assert_eq!(fs::read_to_string(dir.path().join("navigation.toml.bak")).unwrap(), "garbage [[[");
    }

    // §3.2.2: an existing .bak is overwritten by a newer corruption.
    #[test]
    fn existing_bak_is_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        fs::write(dir.path().join("navigation.toml.bak"), "older").unwrap();
        fs::write(&path, "newer [[[").unwrap();
        let r = load(&path).expect("load");
        assert!(r.recovered_from_corruption);
        assert_eq!(fs::read_to_string(dir.path().join("navigation.toml.bak")).unwrap(), "newer [[[");
    }

    // MUST-48(b)/SHOULD-69: a genuine I/O error (not NotFound) is Err, never
    // "empty and OK to save". A directory at the path makes fs::read fail.
    #[test]
    fn non_notfound_io_error_is_propagated_as_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        fs::create_dir(&path).unwrap();
        assert!(load(&path).is_err());
    }

    // MUST-48(b): if the .bak rename itself fails, load returns Err (frontend
    // must then not set hydrated). A directory occupying the .bak path makes
    // renaming a file onto it fail.
    #[test]
    fn backup_rename_failure_is_propagated_as_err() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        fs::write(&path, "garbage [[[").unwrap();
        let bak = dir.path().join("navigation.toml.bak");
        fs::create_dir(&bak).unwrap();
        fs::write(bak.join("keep"), "x").unwrap(); // non-empty dir: rename cannot replace it

        assert!(load(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "garbage [[[", "original must be left intact");
    }

    // R-205-2 / R-248-3 / AC-240: truncate on load to 50/20/20.
    #[test]
    fn oversized_lists_are_truncated_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        let data = NavigationData {
            visit_history: strs("v", 51),
            jump_list: strs("j", 21),
            search_history: strs("s", 21),
        };
        save(&path, &data).expect("save");

        let r = load(&path).expect("load");

        assert_eq!(r.data.visit_history, strs("v", 50));
        assert_eq!(r.data.jump_list, strs("j", 20));
        assert_eq!(r.data.search_history, strs("s", 20));
        assert!(!r.recovered_from_corruption);
    }

    // Boundary: exactly at the limits is untouched.
    #[test]
    fn lists_at_the_limit_are_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        let data = NavigationData {
            visit_history: strs("v", 50),
            jump_list: strs("j", 20),
            search_history: strs("s", 20),
        };
        save(&path, &data).expect("save");
        assert_eq!(load(&path).unwrap().data, data);
    }

    // §3.2.10: the Rust side does NOT dedupe (single TypeScript owner).
    #[test]
    fn rust_side_does_not_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("navigation.toml");
        let data = NavigationData {
            visit_history: vec!["C:\\a".into(), "c:\\a".into()],
            ..Default::default()
        };
        save(&path, &data).expect("save");
        assert_eq!(load(&path).unwrap().data.visit_history.len(), 2);
    }
}
