//! TDD (unit-test-writer, 2026-09-27): `archive::create` — output-path checks and
//! ZIP creation.
//!
//! Specs (SPEC-0300-0014): R-321..R-329, R-322-1, R-323, R-324, R-324-1, R-325
//! (all rows incl. the link row and the surrogate row), R-325-1, R-326 / R-326-3
//! / R-326-4, R-327-1..R-327-11, R-328 / R-328-1 / R-328-2 / R-328-3, R-329,
//! R-346-2 (round trip), R-346-3, R-382, R-383, R-387-2; AC-309, AC-310,
//! AC-310-1, AC-311..AC-317, AC-318..AC-321-5, AC-319-1, AC-353, AC-358-2,
//! AC-364-3 (create half). SPEC-0200-0001 §3.4.3, §3.4.13-1, §3.4.13-2 (MUST-66).
//!
//! Not pinned (spec silent): two items with the same top-level name; the order
//! of entries inside a directory.

use super::create::*;
use super::error::{ArchiveError, ArchiveErrorCode as Code};
use super::extract::{ExtractOutcome, ExtractResult};
use super::progress::ProgressEvent;
use super::test_support::*;
use super::zip_fixture::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

struct Ctx {
    _tmp: tempfile::TempDir,
    /// Base directory (`base_dir`); the ZIP is created here.
    work: PathBuf,
    /// Source material.
    src: PathBuf,
}

fn ctx() -> Ctx {
    let tmp = tempdir();
    let work = tmp.path().join("work");
    let src = tmp.path().join("src");
    fs::create_dir_all(&work).unwrap();
    fs::create_dir_all(&src).unwrap();
    Ctx {
        _tmp: tmp,
        work,
        src,
    }
}

fn create(c: &Ctx, items: &[PathBuf], out: &str) -> Result<CreateResult, ArchiveError> {
    create_zip(items, out, &c.work, false, &CreateHooks::none())
}

fn created(c: &Ctx, items: &[PathBuf], out: &str) -> CreateResult {
    create(c, items, out).unwrap_or_else(|e| panic!("create failed: {e}"))
}

fn names_of(zip: &Path) -> BTreeSet<String> {
    parse_central_directory(&read_bytes(zip))
        .iter()
        .map(|e| e.name_str())
        .collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn read_entry(zip: &Path, name: &str) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip).unwrap()).expect("valid zip");
    let mut f = archive
        .by_name(name)
        .unwrap_or_else(|e| panic!("entry {name}: {e}"));
    let mut out = Vec::new();
    f.read_to_end(&mut out).unwrap();
    out
}

fn code(r: Result<CreateResult, ArchiveError>) -> Code {
    r.expect_err("expected an error").code
}

fn no_output_and_no_temp(dir: &Path) {
    let zips: Vec<String> = tree_names(dir)
        .into_iter()
        .filter(|n| n.to_lowercase().ends_with(".zip"))
        .collect();
    assert!(zips.is_empty(), "no ZIP may be left: {zips:?}");
    assert_no_temp_leftovers(dir);
}

// ================================================= resolve_output_path (pure)

// R-321-adjacent / R-323 / AC-309: `.zip` is appended unless already present (any case).
#[test]
fn ac309_zip_is_appended_unless_the_name_already_ends_with_it() {
    let base = tempdir();
    let b = base.path();
    assert_eq!(resolve_output_path("out", b).unwrap(), b.join("out.zip"));
    assert_eq!(
        resolve_output_path("out.zip", b).unwrap(),
        b.join("out.zip")
    );
    assert_eq!(
        resolve_output_path("out.ZIP", b).unwrap(),
        b.join("out.ZIP"),
        "no `.zip` on top of `.ZIP`"
    );
    assert_eq!(
        resolve_output_path("a.txt", b).unwrap(),
        b.join("a.txt.zip"),
        "R-321: extensions are not replaced"
    );
    assert_eq!(
        resolve_output_path("out.zip.bak", b).unwrap(),
        b.join("out.zip.bak.zip")
    );
}

// R-322-1 / AC-310-1: relative input is resolved against `base_dir`, never the process CWD.
#[test]
fn ac310_1_relative_paths_are_resolved_against_base_dir() {
    let base = tempdir();
    let b = base.path();
    assert_eq!(
        resolve_output_path("out.zip", b).unwrap(),
        b.join("out.zip")
    );
    assert_eq!(
        resolve_output_path(r"sub\x", b).unwrap(),
        b.join("sub").join("x.zip")
    );
    let absolute = b.join("elsewhere").join("y");
    assert_eq!(
        resolve_output_path(&absolute.to_string_lossy(), b).unwrap(),
        b.join("elsewhere").join("y.zip")
    );
}

#[cfg(windows)]
#[test]
fn ac310_1_drive_relative_and_root_relative_paths_are_invalid_path() {
    let base = tempdir();
    for bad in ["C:x.zip", r"\x.zip", "C:out"] {
        assert_eq!(
            resolve_output_path(bad, base.path()).unwrap_err().code,
            Code::InvalidPath,
            "{bad}"
        );
    }
}

#[test]
fn a_base_dir_that_is_not_absolute_is_invalid_path() {
    assert_eq!(
        resolve_output_path("out.zip", Path::new("relative/base"))
            .unwrap_err()
            .code,
        Code::InvalidPath
    );
}

// R-322 / R-324 / AC-310.
#[test]
fn ac310_blank_and_forbidden_names_are_invalid_name() {
    let base = tempdir();
    for bad in ["", "   ", "a?.zip", "a*b", "sub\\a?", "x<y", "out\\"] {
        assert_eq!(
            resolve_output_path(bad, base.path()).unwrap_err().code,
            Code::InvalidName,
            "{bad:?}"
        );
    }
}

// =========================================================== check_create

// R-325 / AC-321-2 .. AC-321-3 and the ordering of the table.
#[test]
fn r325_no_items_and_missing_items() {
    let c = ctx();
    assert_eq!(
        check_create(&[], "out.zip", &c.work).unwrap_err().code,
        Code::NoItems
    );
    let missing = c.src.join("missing.txt");
    assert_eq!(
        check_create(&[missing], "out.zip", &c.work)
            .unwrap_err()
            .code,
        Code::NotFound
    );
}

#[cfg(windows)]
#[test]
fn ac321_3_a_drive_root_item_has_no_archive_name_and_is_invalid_path() {
    let c = ctx();
    let root = PathBuf::from(r"C:\");
    if !root.exists() {
        skip("no C: drive");
        return;
    }
    assert_eq!(
        check_create(&[root.clone()], "out.zip", &c.work)
            .unwrap_err()
            .code,
        Code::InvalidPath
    );
    assert_eq!(
        create(&c, &[root], "out.zip").unwrap_err().code,
        Code::InvalidPath
    );
    no_output_and_no_temp(&c.work);
}

// AC-321-3: an existing directory at the output path -> already_exists.
#[test]
fn ac321_3_an_existing_directory_at_the_output_path_is_already_exists() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    fs::create_dir(c.work.join("out.zip")).unwrap();
    assert_eq!(
        check_create(&[item.clone()], "out.zip", &c.work)
            .unwrap_err()
            .code,
        Code::AlreadyExists
    );
    assert_eq!(
        create(&c, &[item], "out.zip").unwrap_err().code,
        Code::AlreadyExists
    );
}

// R-324-1: a missing parent directory is refused (never auto-created).
#[test]
fn r324_1_a_missing_parent_directory_is_destination_not_found() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let out = c.work.join("no").join("such").join("out.zip");
    assert_eq!(
        check_create(&[item.clone()], &out.to_string_lossy(), &c.work)
            .unwrap_err()
            .code,
        Code::DestinationNotFound
    );
    assert_eq!(
        create(&c, &[item], &out.to_string_lossy())
            .unwrap_err()
            .code,
        Code::DestinationNotFound
    );
    assert!(
        !c.work.join("no").exists(),
        "no directory may be auto-created"
    );
}

// AC-321-2: output inside a source directory.
#[test]
fn ac321_2_an_output_path_inside_a_source_directory_is_destination_inside_source() {
    let c = ctx();
    let docs = c.src.join("docs");
    write_file(&docs, "x.txt", b"x");
    let out = docs.join("out.zip");
    assert_eq!(
        check_create(&[docs.clone()], &out.to_string_lossy(), &c.work)
            .unwrap_err()
            .code,
        Code::DestinationInsideSource
    );
    assert_eq!(
        create(&c, &[docs.clone()], &out.to_string_lossy())
            .unwrap_err()
            .code,
        Code::DestinationInsideSource
    );
    assert!(!out.exists());
    assert_no_temp_leftovers(&docs);
}

// R-325: output identical to a source FILE.
#[test]
fn r325_an_output_path_identical_to_a_source_file_is_destination_inside_source() {
    let c = ctx();
    let precious = write_file(
        &c.src,
        "keep.zip",
        b"i am a source file that happens to be named like the output",
    );
    let err = create_zip(
        &[precious.clone()],
        &precious.to_string_lossy(),
        &c.work,
        true,
        &CreateHooks::none(),
    )
    .unwrap_err();
    assert_eq!(err.code, Code::DestinationInsideSource);
    assert_eq!(
        read_string(&precious),
        "i am a source file that happens to be named like the output"
    );
}

// A sibling whose NAME merely starts like the source is not inside it.
#[test]
fn a_sibling_directory_with_a_common_name_prefix_is_not_inside_the_source() {
    let c = ctx();
    let docs = c.src.join("docs");
    write_file(&docs, "x.txt", b"x");
    let sibling = c.src.join("docs2");
    fs::create_dir(&sibling).unwrap();
    let out = sibling.join("out.zip");
    let result = created_at(&c, &[docs], &out);
    assert!(result.output_path.ends_with("out.zip"));
}

fn created_at(c: &Ctx, items: &[PathBuf], out: &Path) -> CreateResult {
    created(c, items, &out.to_string_lossy())
}

// AC-321-4 (a): letter-case spelling (Windows).
#[cfg(windows)]
#[test]
fn ac321_4_a_letter_case_alias_of_a_source_directory_is_still_inside_it() {
    let c = ctx();
    let docs = c.src.join("Docs");
    write_file(&docs, "x.txt", b"x");
    let out = c.src.join("DOCS").join("x.zip");
    assert_eq!(
        check_create(&[docs.clone()], &out.to_string_lossy(), &c.work)
            .unwrap_err()
            .code,
        Code::DestinationInsideSource
    );
    assert!(!docs.join("x.zip").exists());
}

// AC-321-4 (b): a junction that leads into the source.
#[test]
fn ac321_4_a_junction_alias_into_a_source_directory_is_still_inside_it() {
    let c = ctx();
    let docs = c.src.join("docs");
    write_file(&docs, "x.txt", b"x");
    let link = c.work.join("link");
    if !make_dir_link(&link, &docs) {
        skip("cannot create a junction/symlink here");
        return;
    }
    let out = link.join("x.zip");
    assert_eq!(
        check_create(&[docs.clone()], &out.to_string_lossy(), &c.work)
            .unwrap_err()
            .code,
        Code::DestinationInsideSource
    );
    assert_eq!(
        create(&c, &[docs.clone()], &out.to_string_lossy())
            .unwrap_err()
            .code,
        Code::DestinationInsideSource
    );
    assert!(!docs.join("x.zip").exists());
    remove_dir_link(&link);
}

// AC-321-5 / R-325 link row: an existing link at the output path (with or without
// a target) is `invalid_path`; no overwrite question, nothing changed.
#[test]
fn ac321_5_an_output_path_that_is_a_link_is_invalid_path() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    // healthy directory junction
    let real_dir = c.work.join("real-dir");
    fs::create_dir(&real_dir).unwrap();
    let link = c.work.join("out.zip");
    if !make_dir_link(&link, &real_dir) {
        skip("cannot create a junction/symlink here");
        return;
    }
    assert_eq!(
        check_create(&[item.clone()], "out.zip", &c.work)
            .unwrap_err()
            .code,
        Code::InvalidPath
    );
    let err = create_zip(
        &[item.clone()],
        "out.zip",
        &c.work,
        true,
        &CreateHooks::none(),
    )
    .unwrap_err();
    assert_eq!(
        err.code,
        Code::InvalidPath,
        "even with overwrite=true a link is never a valid target"
    );
    assert!(
        tree_names(&real_dir).is_empty(),
        "nothing may be written through the link"
    );
    remove_dir_link(&link);

    // dangling junction
    assert!(make_dangling_dir_link(&link, &c.work.join("scratch")));
    assert_eq!(
        check_create(&[item.clone()], "out.zip", &c.work)
            .unwrap_err()
            .code,
        Code::InvalidPath
    );
    assert!(!c.work.join("scratch").exists());
    remove_dir_link(&link);
}

#[test]
fn ac321_5_a_file_symlink_at_the_output_path_is_invalid_path_and_its_target_unchanged() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let victim = write_file(&c.work, "victim.zip", b"victim original");
    let link = c.work.join("out.zip");
    if !make_file_symlink(&link, &victim) {
        skip("file symlinks need the symlink privilege on Windows");
        return;
    }
    let err = create_zip(&[item], "out.zip", &c.work, true, &CreateHooks::none()).unwrap_err();
    assert_eq!(err.code, Code::InvalidPath);
    assert_eq!(read_string(&victim), "victim original");
}

// R-325 last row / AC-364-3 (create half): a name with an unpaired surrogate.
#[cfg(windows)]
#[test]
fn ac364_3_a_source_name_with_an_unpaired_surrogate_is_invalid_path() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let c = ctx();
    let dir = c.src.join("holder");
    fs::create_dir(&dir).unwrap();
    let bad_name = OsString::from_wide(&[
        b'b' as u16,
        b'a' as u16,
        b'd' as u16,
        0xD800,
        b'.' as u16,
        b't' as u16,
    ]);
    let bad = dir.join(&bad_name);
    if fs::write(&bad, b"x").is_err() {
        skip("the filesystem refused a name with an unpaired surrogate");
        return;
    }
    let out = c.work.join("out.zip");
    assert_eq!(
        create(&c, &[bad.clone()], &out.to_string_lossy())
            .unwrap_err()
            .code,
        Code::InvalidPath,
        "as a top-level item"
    );
    assert_eq!(
        create(&c, &[dir.clone()], &out.to_string_lossy())
            .unwrap_err()
            .code,
        Code::InvalidPath,
        "inside a directory"
    );
    no_output_and_no_temp(&c.work);
}

// check_create is side-effect free in every outcome.
#[test]
fn check_create_never_touches_the_filesystem() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    write_file(&c.work, "exists.zip", b"old zip");
    let before = (tree(&c.work), tree(&c.src));
    let ok = check_create(&[item.clone()], "fresh", &c.work).unwrap();
    assert_eq!(
        ok,
        CheckedCreate {
            output_path: c.work.join("fresh.zip"),
            overwrite_required: false
        }
    );
    let existing = check_create(&[item.clone()], "exists.zip", &c.work).unwrap();
    assert!(
        existing.overwrite_required,
        "an existing FILE needs the overwrite confirmation (R-326)"
    );
    assert_eq!(existing.output_path, c.work.join("exists.zip"));
    let _ = check_create(&[item], "no\\parent\\x.zip", &c.work);
    assert_eq!((tree(&c.work), tree(&c.src)), before);
}

// ================================================================ contents

// AC-300 / R-327-1: each item is stored under its own name (no parent path).
#[test]
fn r327_1_items_are_stored_under_their_own_names() {
    let c = ctx();
    let a = write_file(&c.src, "a.txt", b"alpha");
    let b = write_file(&c.src.join("nested"), "b.txt", b"beta");
    let docs = c.src.join("docs");
    write_file(&docs, "d.txt", b"delta");
    let r = created(&c, &[a, b, docs], "out");
    assert_eq!(r.output_path, c.work.join("out.zip"));
    assert_eq!(r.entries, 4, "a.txt, b.txt, docs/, docs/d.txt");
    assert_eq!(r.skipped_links, 0);
    let zip = c.work.join("out.zip");
    assert_eq!(
        names_of(&zip),
        set(&["a.txt", "b.txt", "docs/", "docs/d.txt"])
    );
    assert_eq!(read_entry(&zip, "a.txt"), b"alpha");
    assert_eq!(read_entry(&zip, "docs/d.txt"), b"delta");
    assert_no_temp_leftovers(&c.work);
}

// AC-318 / R-327-2 / R-327-3: hierarchy, empty directories, `/` separators; and the
// extraction round trip reproduces the tree.
#[test]
fn ac318_a_folder_round_trips_through_create_and_extract() {
    let c = ctx();
    let docs = c.src.join("docs");
    write_file(&docs, "top.txt", b"top");
    write_file(&docs, "sub/deep/x.txt", b"deep");
    fs::create_dir_all(docs.join("empty")).unwrap();
    created(&c, &[docs.clone()], "docs");
    let zip = c.work.join("docs.zip");
    let names = names_of(&zip);
    assert_eq!(
        names,
        set(&[
            "docs/",
            "docs/top.txt",
            "docs/sub/",
            "docs/sub/deep/",
            "docs/sub/deep/x.txt",
            "docs/empty/"
        ])
    );
    assert!(names.iter().all(|n| !n.contains('\\')), "R-327-3: `/` only");
    let out = c.work.join("restored");
    let r: ExtractResult = extract_simple(&zip, &out, &c.work).expect("extract");
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    assert_eq!(
        tree(&out),
        tree(&c.src),
        "the restored tree equals the source tree"
    );
    assert!(out.join("docs/empty").is_dir());
}

// AC-319 / R-327-4 / R-327-5: Deflate for files, Stored for directories, never encrypted.
#[test]
fn ac319_files_are_deflated_directories_stored_and_nothing_is_encrypted() {
    let c = ctx();
    let docs = c.src.join("docs");
    write_file(&docs, "a.txt", &vec![b'a'; 10_000]);
    fs::create_dir_all(docs.join("empty")).unwrap();
    created(&c, &[docs], "out");
    let cd = parse_central_directory(&read_bytes(&c.work.join("out.zip")));
    assert!(cd.len() >= 3);
    for e in &cd {
        assert_eq!(
            e.flags & FLAG_ENCRYPTED,
            0,
            "{} must not be encrypted",
            e.name_str()
        );
        if e.name.ends_with(b"/") {
            assert_eq!(e.method, METHOD_STORED, "{}", e.name_str());
        } else {
            assert_eq!(e.method, METHOD_DEFLATE, "{}", e.name_str());
        }
    }
}

// AC-320 / R-327-6: UTF-8 names with the language-encoding flag (bit 11).
#[test]
fn ac320_japanese_names_are_utf8_with_the_bit_11_flag() {
    let c = ctx();
    let item = write_file(&c.src, "日本語のファイル.txt", "こんにちは".as_bytes());
    let folder = c.src.join("資料");
    write_file(&folder, "メモ.txt", b"memo");
    created(&c, &[item, folder], "out");
    let cd = parse_central_directory(&read_bytes(&c.work.join("out.zip")));
    let expect = ["日本語のファイル.txt", "資料/", "資料/メモ.txt"];
    for name in expect {
        let e = cd
            .iter()
            .find(|e| e.name == name.as_bytes())
            .unwrap_or_else(|| {
                panic!(
                    "{name} missing: {:?}",
                    cd.iter().map(|e| e.name_str()).collect::<Vec<_>>()
                )
            });
        assert_eq!(e.flags & FLAG_UTF8, FLAG_UTF8, "{name}: bit 11 must be set");
    }
    assert_eq!(
        read_entry(&c.work.join("out.zip"), "日本語のファイル.txt"),
        "こんにちは".as_bytes()
    );
}

// AC-321 / R-327-7: hidden files are included regardless of any UI setting.
#[test]
fn ac321_hidden_files_are_included() {
    let c = ctx();
    let folder = c.src.join("proj");
    let dotfile = write_file(&folder, ".env", b"SECRET=1");
    let hidden = write_file(&folder, "hidden.txt", b"h");
    #[cfg(windows)]
    {
        for p in [&dotfile, &hidden] {
            let status = std::process::Command::new("attrib")
                .arg("+h")
                .arg(p)
                .status()
                .expect("attrib");
            assert!(status.success());
        }
    }
    let _ = (&dotfile, &hidden);
    created(&c, &[folder], "out");
    assert_eq!(
        names_of(&c.work.join("out.zip")),
        set(&["proj/", "proj/.env", "proj/hidden.txt"])
    );
}

// AC-321-1 / R-327-8: links are skipped and counted; a loop cannot hang the run.
#[test]
fn ac321_1_junctions_are_skipped_counted_and_a_cycle_terminates() {
    let c = ctx();
    let src = c.src.join("proj");
    write_file(&src, "real/x.txt", b"x");
    let other = c.src.join("other");
    write_file(&other, "y.txt", b"y");
    if !make_dir_link(&src.join("to-other"), &other) || !make_dir_link(&src.join("loop"), &src) {
        skip("cannot create junctions/symlinks here");
        return;
    }
    let r = created(&c, &[src.clone()], "out");
    let names = names_of(&c.work.join("out.zip"));
    assert_eq!(
        names,
        set(&["proj/", "proj/real/", "proj/real/x.txt"]),
        "links and everything behind them are left out"
    );
    assert_eq!(r.skipped_links, 2);
    assert_eq!(r.entries, 3);
    remove_dir_link(&src.join("to-other"));
    remove_dir_link(&src.join("loop"));
}

// A link given as a TOP-LEVEL item is skipped as well (never followed).
#[test]
fn r327_8_a_link_item_is_skipped_not_followed() {
    let c = ctx();
    let real = c.src.join("real");
    write_file(&real, "x.txt", b"x");
    let plain = write_file(&c.src, "plain.txt", b"p");
    let link = c.src.join("link");
    if !make_dir_link(&link, &real) {
        skip("cannot create a junction/symlink here");
        return;
    }
    let r = created(&c, &[plain, link.clone()], "out");
    assert_eq!(names_of(&c.work.join("out.zip")), set(&["plain.txt"]));
    assert_eq!(r.skipped_links, 1);
    remove_dir_link(&link);
}

// R-327-11 / AC-319-1 (create half): the ZIP records the source modification time.
#[test]
fn r327_11_the_modification_time_is_recorded_in_the_archive() {
    let c = ctx();
    let f = write_file(&c.src, "dated.txt", b"x");
    set_mtime(&f, unix_time(1_592_222_400)).expect("set mtime"); // 2020-06-15 12:00:00 UTC
    created(&c, &[f], "out");
    let cd = parse_central_directory(&read_bytes(&c.work.join("out.zip")));
    let year = 1980 + (cd[0].dos_date >> 9);
    let month = (cd[0].dos_date >> 5) & 0xF;
    assert_eq!((year, month), (2020, 6), "not the 1980-01-01 default");
}

// R-327-11: out-of-range times are clamped, never an error.
#[test]
fn r327_11_out_of_range_times_are_clamped_not_failed() {
    let c = ctx();
    let old = write_file(&c.src, "old.txt", b"o");
    let future = write_file(&c.src, "future.txt", b"f");
    if set_mtime(&old, unix_time(0)).is_err()
        || set_mtime(&future, unix_time(7_258_118_400)).is_err()
    {
        skip("the filesystem refused the extreme timestamps");
        return;
    }
    created(&c, &[old, future], "out");
    let cd = parse_central_directory(&read_bytes(&c.work.join("out.zip")));
    for e in &cd {
        let year = 1980 + (e.dos_date >> 9);
        assert!(
            (1980..=2107).contains(&year),
            "{}: year {year}",
            e.name_str()
        );
    }
    let old_entry = cd.iter().find(|e| e.name == b"old.txt").unwrap();
    assert!(
        1980 + (old_entry.dos_date >> 9) <= 1980,
        "1970 is rounded up to the 1980 epoch"
    );
}

// AC-319-1: create -> extract restores file AND directory times (2 s tolerance) and
// does not carry the read-only attribute over (R-346-3).
#[test]
fn ac319_1_modification_times_survive_a_round_trip_and_readonly_is_not_applied() {
    let c = ctx();
    let folder = c.src.join("proj");
    let file = write_file(&folder, "sub/f.txt", b"data");
    let sub = folder.join("sub");
    let when_file = unix_time(1_577_934_246); // 2020-01-02 03:04:06 UTC (even seconds)
    let when_dir = unix_time(1_557_126_490); // 2019-05-06 07:08:10 UTC
    set_mtime(&file, when_file).unwrap();
    set_mtime(&sub, when_dir).unwrap();
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();
    created(&c, &[folder], "out");
    let out = c.work.join("restored");
    let r = extract_simple(&c.work.join("out.zip"), &out, &c.work).expect("extract");
    assert_eq!(r.outcome, ExtractOutcome::Completed, "{:?}", r.error);
    let restored_file = out.join("proj/sub/f.txt");
    assert!(
        secs_apart(mtime(&restored_file), when_file) <= 2,
        "file time restored"
    );
    assert!(
        secs_apart(mtime(&out.join("proj/sub")), when_dir) <= 2,
        "directory time restored (set after its children)"
    );
    assert!(
        !fs::metadata(&restored_file)
            .unwrap()
            .permissions()
            .readonly(),
        "R-346-3"
    );
    // so that the temp dir can be removed
    let mut p = fs::metadata(&file).unwrap().permissions();
    p.set_readonly(false);
    fs::set_permissions(&file, p).unwrap();
}

// R-327-9: the large-file (ZIP64) decision boundary; nothing is actually 4 GiB here.
#[test]
fn r327_9_large_file_option_starts_at_u32_max() {
    assert!(!needs_large_file(0));
    assert!(!needs_large_file(1 << 20));
    assert!(!needs_large_file(u32::MAX as u64 - 1));
    assert!(needs_large_file(u32::MAX as u64));
    assert!(needs_large_file(5 << 30));
}

// ==================================================== overwrite / conflict / MUST-66

// AC-311 / R-326: the pre-check reports whether confirmation is needed.
#[test]
fn ac311_check_create_flags_an_existing_file_for_confirmation() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    assert!(
        !check_create(&[item.clone()], "fresh.zip", &c.work)
            .unwrap()
            .overwrite_required
    );
    write_file(&c.work, "taken.zip", b"old");
    assert!(
        check_create(&[item], "taken.zip", &c.work)
            .unwrap()
            .overwrite_required
    );
}

// AC-314 / R-326-3: no instruction to overwrite -> `conflict`, nothing touched.
#[test]
fn ac314_an_existing_output_without_overwrite_is_conflict_and_unchanged() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let existing = write_file(&c.work, "out.zip", b"precious old zip");
    assert_eq!(code(create(&c, &[item], "out.zip")), Code::Conflict);
    assert_eq!(read_string(&existing), "precious old zip");
    assert_no_temp_leftovers(&c.work);
}

// AC-313 / R-326-4: overwrite=true replaces it with the new ZIP.
#[test]
fn ac313_overwrite_replaces_the_existing_output() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"new content");
    write_file(&c.work, "out.zip", b"old, not even a zip");
    let r = create_zip(&[item], "out.zip", &c.work, true, &CreateHooks::none()).expect("create");
    assert_eq!(r.entries, 1);
    assert_eq!(read_entry(&c.work.join("out.zip"), "a.txt"), b"new content");
    assert_no_temp_leftovers(&c.work);
}

// MUST-66 (§3.4.13-2) / R-326-3: overwrite=false + a file that appears just before
// the commit is NOT replaced; `conflict`; temp file removed.
#[test]
fn must66_an_output_that_appears_before_the_commit_is_never_replaced() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let intruder = |final_path: &Path| {
        fs::write(final_path, "created by someone else").expect("racing file");
    };
    let hooks = CreateHooks {
        before_commit: &intruder,
        ..CreateHooks::none()
    };
    let err = create_zip(&[item], "out.zip", &c.work, false, &hooks).unwrap_err();
    assert_eq!(err.code, Code::Conflict);
    assert_eq!(
        read_string(&c.work.join("out.zip")),
        "created by someone else"
    );
    assert_no_temp_leftovers(&c.work);
}

// AC-315 / R-328-1: a source that cannot be read stops the run; no output, no temp.
#[cfg(windows)]
#[test]
fn ac315_an_unreadable_source_leaves_no_output_and_no_temp_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let c = ctx();
    let folder = c.src.join("proj");
    write_file(&folder, "a.txt", b"a");
    let locked = write_file(&folder, "locked.txt", b"locked");
    write_file(&folder, "z.txt", b"z");
    let _exclusive = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&locked)
        .expect("lock");
    let err = create(&c, &[folder], "out.zip").unwrap_err();
    assert!(
        matches!(err.code, Code::IoError | Code::PermissionDenied),
        "{:?}",
        err.code
    );
    no_output_and_no_temp(&c.work);
}

// AC-316 / R-326-4 / R-328-2: a failed/canceled OVERWRITE leaves the old file as it was.
#[test]
fn ac316_a_canceled_overwrite_keeps_the_existing_file_and_leaves_no_temp() {
    let c = ctx();
    let folder = c.src.join("proj");
    for i in 0..5 {
        write_file(&folder, &format!("f{i}.txt"), b"data");
    }
    let existing = write_file(&c.work, "out.zip", b"the original archive bytes");
    let flag = AtomicBool::new(false);
    let raise = |ev: &ProgressEvent| {
        if ev.index == 3 {
            flag.store(true, Ordering::Relaxed);
        }
    };
    let poll = || flag.load(Ordering::Relaxed);
    let hooks = CreateHooks {
        should_cancel: &poll,
        on_progress: &raise,
        ..CreateHooks::none()
    };
    let err = create_zip(&[folder], "out.zip", &c.work, true, &hooks).unwrap_err();
    assert_eq!(err.code, Code::Canceled);
    assert_eq!(read_string(&existing), "the original archive bytes");
    assert_no_temp_leftovers(&c.work);
}

// AC-353 / R-383 / R-328-2: cancel -> `canceled`, no output, no temp.
#[test]
fn ac353_a_canceled_creation_leaves_nothing_behind() {
    let c = ctx();
    let folder = c.src.join("proj");
    for i in 0..5 {
        write_file(&folder, &format!("f{i}.txt"), b"data");
    }
    let flag = AtomicBool::new(false);
    let raise = |ev: &ProgressEvent| {
        if ev.index == 2 {
            flag.store(true, Ordering::Relaxed);
        }
    };
    let poll = || flag.load(Ordering::Relaxed);
    let hooks = CreateHooks {
        should_cancel: &poll,
        on_progress: &raise,
        ..CreateHooks::none()
    };
    let err = create_zip(&[folder], "out.zip", &c.work, false, &hooks).unwrap_err();
    assert_eq!(err.code, Code::Canceled);
    no_output_and_no_temp(&c.work);
}

// AC-358-2 / R-387-2: canceled before anything started -> nothing exists, not even a temp.
#[test]
fn ac358_2_a_cancel_requested_up_front_creates_nothing() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let hooks = CreateHooks {
        should_cancel: &|| true,
        ..CreateHooks::none()
    };
    let err = create_zip(&[item], "out.zip", &c.work, false, &hooks).unwrap_err();
    assert_eq!(err.code, Code::Canceled);
    assert!(tree_names(&c.work).is_empty(), "{:?}", tree_names(&c.work));
}

// AC-317 / R-328: while entries are being written the FINAL name does not exist and
// a fixed-shape temp file does; progress is 1-based per entry with the archive path.
#[test]
fn ac317_the_output_is_written_to_a_temp_file_until_the_end() {
    let c = ctx();
    let folder = c.src.join("proj");
    write_file(&folder, "a.txt", b"a");
    write_file(&folder, "b.txt", b"b");
    let out = c.work.join("out.zip");
    let events: RefCell<Vec<(String, usize, usize)>> = RefCell::new(Vec::new());
    let temps_seen: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    let observe = |ev: &ProgressEvent| {
        assert!(
            !out.exists(),
            "the final name must not exist while writing (entry {})",
            ev.index
        );
        let temps = tree_names(&c.work)
            .into_iter()
            .filter(|n| n.starts_with(".rfz-") && n.ends_with(".tmp"))
            .count();
        temps_seen.borrow_mut().push(temps);
        events
            .borrow_mut()
            .push((ev.name.to_string(), ev.index, ev.total));
    };
    let hooks = CreateHooks {
        on_progress: &observe,
        ..CreateHooks::none()
    };
    let r = create_zip(&[folder], "out.zip", &c.work, false, &hooks).expect("create");
    assert!(out.exists());
    assert_no_temp_leftovers(&c.work);
    let events = events.borrow();
    assert_eq!(events.len(), r.entries, "one notification per entry");
    assert_eq!(r.entries, 3, "proj/, proj/a.txt, proj/b.txt");
    for (i, (_, index, total)) in events.iter().enumerate() {
        assert_eq!((*index, *total), (i + 1, 3));
    }
    assert!(
        events.iter().all(|(n, _, _)| n.starts_with("proj")),
        "{events:?}"
    );
    assert!(
        temps_seen.borrow().iter().all(|&n| n == 1),
        "exactly one temp file during the run: {:?}",
        temps_seen.borrow()
    );
}

// R-328-3: leftovers from a crashed earlier run are NOT cleaned up.
#[test]
fn r328_3_stale_temp_files_from_earlier_runs_are_left_alone() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let stale = write_file(&c.work, ".rfz-1-1.tmp", b"stale");
    created(&c, &[item], "out");
    assert_eq!(read_string(&stale), "stale");
}

// R-324 / R-322-1 end to end: `out` -> `<base>/out.zip`.
#[test]
fn ac309_create_zip_appends_the_extension_to_the_created_file() {
    let c = ctx();
    let item = write_file(&c.src, "a.txt", b"a");
    let r = created(&c, &[item], "out");
    assert_eq!(r.output_path, c.work.join("out.zip"));
    assert!(c.work.join("out.zip").is_file());
    assert!(!c.work.join("out").exists());
}
