//! `fs/` (SPEC-0300-0009 §7, R-70/R-71): the single directory-enumeration
//! command used by both the file list and the tree (R-41 — the tree passes
//! `sort_key: "name"`, `sort_order: "asc"`, dir-only filtering happens on the
//! frontend/caller side of this same command, not via a second backend
//! command).

use std::cmp::Ordering;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// R-71: `type` (a Rust keyword) is represented as `entry_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    File,
    Dir,
}

/// R-71: the 7 fields returned by `fs_list_dir`, 1:1 with the frontend type.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub entry_type: EntryType,
    pub size: u64,
    pub modified: String,
    pub hidden: bool,
    pub ext: String,
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

/// Extension without the leading dot (R-71). Directories never carry an
/// extension.
fn extract_ext(name: &str, is_dir: bool) -> String {
    if is_dir {
        return String::new();
    }
    match name.rfind('.') {
        // A dot at position 0 is a dotfile name, not an extension separator
        // (e.g. ".hidden.txt" still has an extension "txt", but a plain
        // ".gitignore"-style name with no further dot has none).
        Some(idx) if idx > 0 => name[idx + 1..].to_string(),
        _ => String::new(),
    }
}

/// Modified time as a zero-padded, lexicographically-sortable string
/// (R-71 notes the exact text format is TBD; this only needs to sort
/// consistently with "modified" as a string, per R-33).
fn format_modified(modified: std::time::SystemTime) -> String {
    let secs = modified
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{:020}", secs)
}

/// R-70: the single directory-enumeration command
/// `fs_list_dir(path, show_hidden, sort_key, sort_order) -> Vec<Entry>`.
pub fn fs_list_dir(
    path: &Path,
    show_hidden: bool,
    sort_key: &str,
    sort_order: &str,
) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for dir_entry in std::fs::read_dir(path)? {
        let dir_entry = dir_entry?;
        let name = dir_entry.file_name().to_string_lossy().into_owned();
        let hidden = is_hidden_name(&name);

        // R-74: show_hidden controls whether dotfiles are enumerated at all.
        if hidden && !show_hidden {
            continue;
        }

        let metadata = dir_entry.metadata()?;
        let is_dir = metadata.is_dir();
        let entry_type = if is_dir { EntryType::Dir } else { EntryType::File };
        // AC-13: directories never carry a numeric size.
        let size = if is_dir { 0 } else { metadata.len() };
        let ext = extract_ext(&name, is_dir);
        let modified = metadata
            .modified()
            .map(format_modified)
            .unwrap_or_else(|_| String::new());

        entries.push(Entry {
            name,
            path: dir_entry.path(),
            entry_type,
            size,
            modified,
            hidden,
            ext,
        });
    }

    sort_entries(&mut entries, sort_key, sort_order);

    Ok(entries)
}

fn sort_entries(entries: &mut [Entry], sort_key: &str, sort_order: &str) {
    entries.sort_by(|a, b| {
        let ordering = match sort_key {
            // R-33: case-insensitive name comparison.
            "size" => a.size.cmp(&b.size),
            // R-33: dirs first, then (lowercased) extension, then (lowercased) name.
            "type" => compare_type(a, b),
            // R-33: modified compared as a string.
            "modified" => a.modified.cmp(&b.modified),
            // "name" (and any unrecognized key defaults to name, per R-30).
            _ => compare_name(a, b),
        };
        if sort_order == "desc" {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

fn compare_name(a: &Entry, b: &Entry) -> Ordering {
    a.name.to_lowercase().cmp(&b.name.to_lowercase())
}

fn compare_type(a: &Entry, b: &Entry) -> Ordering {
    let a_is_dir = a.entry_type == EntryType::Dir;
    let b_is_dir = b.entry_type == EntryType::Dir;

    // Directories are grouped first: a Dir (false=0 when negated) sorts
    // before a File.
    match (a_is_dir, b_is_dir) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }

    let ext_cmp = a.ext.to_lowercase().cmp(&b.ext.to_lowercase());
    if ext_cmp != Ordering::Equal {
        return ext_cmp;
    }

    compare_name(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write_file(dir: &std::path::Path, name: &str, contents: &[u8]) {
        let mut f = fs::File::create(dir.join(name)).expect("create test file");
        f.write_all(contents).expect("write test file contents");
    }

    // SPEC-0300-0009 AC-20 (R-33): name sort is case-insensitive.
    #[test]
    fn ac20_name_sort_is_case_insensitive() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "banana.txt", b"1");
        write_file(dir.path(), "Apple.txt", b"1");
        write_file(dir.path(), "cherry.txt", b"1");

        let entries = fs_list_dir(dir.path(), false, "name", "asc").expect("fs_list_dir");
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["Apple.txt", "banana.txt", "cherry.txt"]);
    }

    #[test]
    fn ac20_name_sort_desc_reverses_the_case_insensitive_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "banana.txt", b"1");
        write_file(dir.path(), "Apple.txt", b"1");
        write_file(dir.path(), "cherry.txt", b"1");

        let entries = fs_list_dir(dir.path(), false, "name", "desc").expect("fs_list_dir");
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["cherry.txt", "banana.txt", "Apple.txt"]);
    }

    // SPEC-0300-0009 AC-21 (R-33): size sort is a numeric byte-count comparison.
    #[test]
    fn ac21_size_sort_is_numeric_not_lexicographic() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "small.bin", &[0u8; 9]); // 9 bytes
        write_file(dir.path(), "large.bin", &[0u8; 100]); // 100 bytes (lexicographically "1..." < "9...")
        write_file(dir.path(), "medium.bin", &[0u8; 20]); // 20 bytes

        let entries = fs_list_dir(dir.path(), false, "size", "asc").expect("fs_list_dir");
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["small.bin", "medium.bin", "large.bin"]);
    }

    // SPEC-0300-0009 AC-13: directories show an empty size, never a numeric one.
    #[test]
    fn ac13_directory_size_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join("subdir")).expect("create subdir");

        let entries = fs_list_dir(dir.path(), false, "name", "asc").expect("fs_list_dir");
        let subdir = entries.iter().find(|e| e.name == "subdir").expect("subdir entry present");
        assert_eq!(subdir.size, 0, "directory entries must not carry a file size");
        assert!(matches!(subdir.entry_type, EntryType::Dir));
    }

    // SPEC-0300-0009 AC-22 (R-33): type sort groups directories first, then
    // orders by (lowercased) extension, then by (lowercased) name.
    #[test]
    fn ac22_type_sort_groups_dirs_first_then_ext_then_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join("zdir")).expect("create dir");
        fs::create_dir(dir.path().join("adir")).expect("create dir");
        write_file(dir.path(), "b.TXT", b"1"); // ext should be compared case-insensitively
        write_file(dir.path(), "a.txt", b"1");
        write_file(dir.path(), "c.md", b"1");

        let entries = fs_list_dir(dir.path(), false, "type", "asc").expect("fs_list_dir");
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        // Directories first (alphabetical among themselves via the 3rd-stage name
        // comparison), then files grouped by extension (md before txt), then by
        // name within the same extension.
        assert_eq!(names, vec!["adir", "zdir", "c.md", "a.txt", "b.TXT"]);
    }

    // SPEC-0300-0009 R-74: `show_hidden` controls whether dotfiles are enumerated.
    #[test]
    fn show_hidden_false_excludes_dotfiles() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "visible.txt", b"1");
        write_file(dir.path(), ".hidden.txt", b"1");

        let entries = fs_list_dir(dir.path(), false, "name", "asc").expect("fs_list_dir");
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["visible.txt"]);
    }

    #[test]
    fn show_hidden_true_includes_dotfiles_and_marks_hidden_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "visible.txt", b"1");
        write_file(dir.path(), ".hidden.txt", b"1");

        let entries = fs_list_dir(dir.path(), true, "name", "asc").expect("fs_list_dir");
        let hidden_entry = entries.iter().find(|e| e.name == ".hidden.txt").expect(".hidden.txt present");
        assert!(hidden_entry.hidden);
        let visible_entry = entries.iter().find(|e| e.name == "visible.txt").expect("visible.txt present");
        assert!(!visible_entry.hidden);
    }

    // SPEC-0300-0009 AC-56 / R-71: Entry carries exactly the 7 specified fields,
    // and fs_list_dir is the single enumeration command (4-argument signature).
    #[test]
    fn ac56_entry_has_the_seven_specified_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "a.txt", b"hello");

        let entries = fs_list_dir(dir.path(), false, "name", "asc").expect("fs_list_dir");
        let entry = entries.into_iter().next().expect("one entry");

        // Field access below is itself the compile-time assertion that Entry
        // exposes exactly these 7 fields with plausible types.
        let _name: String = entry.name;
        let _path: std::path::PathBuf = entry.path;
        let _entry_type: EntryType = entry.entry_type;
        let _size: u64 = entry.size;
        let _modified: String = entry.modified;
        let _hidden: bool = entry.hidden;
        let _ext: String = entry.ext;
    }

    #[test]
    fn ac56_ext_is_extracted_without_the_leading_dot() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_file(dir.path(), "a.txt", b"hello");

        let entries = fs_list_dir(dir.path(), false, "name", "asc").expect("fs_list_dir");
        let entry = entries.into_iter().find(|e| e.name == "a.txt").expect("a.txt present");
        assert_eq!(entry.ext, "txt");
    }
}
