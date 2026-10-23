//! TDD (unit-test-writer, 2026-09-27): `archive::progress` — event name, payload
//! DTO and the emit throttle.
//!
//! Specs: SPEC-0300-0014 R-381 (entry name + current/total), R-386 (event name
//! `archive://progress`, distinct from `fs_ops://progress`); SPEC-0200-0001
//! §3.4.1 (`ArchiveProgressDto { operationId, name, index, total }`), §3.4.5
//! (first / last / >= 50 ms), SHOULD-88.

use super::progress::*;
use std::time::Duration;

#[test]
fn r386_event_name_is_archive_progress() {
    assert_eq!(PROGRESS_EVENT_NAME, "archive://progress");
    assert_ne!(PROGRESS_EVENT_NAME, "fs_ops://progress");
}

#[test]
fn dto_serializes_as_camel_case() {
    let dto = ArchiveProgressDto {
        operation_id: "op-1".into(),
        name: "dir/a.txt".into(),
        index: 2,
        total: 5,
    };
    let text = toml::to_string(&dto).expect("serialize");
    for key in [
        "operationId = \"op-1\"",
        "name = \"dir/a.txt\"",
        "index = 2",
        "total = 5",
    ] {
        assert!(text.contains(key), "missing `{key}` in:\n{text}");
    }
    assert!(!text.contains("operation_id"), "{text}");
}

#[test]
fn throttle_interval_is_50_ms() {
    assert_eq!(EMIT_INTERVAL, Duration::from_millis(50));
}

// §3.4.5: the first entry is always emitted.
#[test]
fn first_entry_is_always_emitted() {
    assert!(should_emit(1, 1000, None));
    assert!(should_emit(1, 1000, Some(Duration::from_millis(0))));
}

// §3.4.5: the last entry is always emitted, however soon after the previous one.
#[test]
fn last_entry_is_always_emitted() {
    assert!(should_emit(1000, 1000, Some(Duration::from_millis(0))));
    assert!(should_emit(1, 1, None));
}

// Nothing has been emitted yet -> emit (even for a middle entry).
#[test]
fn nothing_emitted_yet_means_emit() {
    assert!(should_emit(7, 1000, None));
}

// R-381 / IPC flood control: a middle entry is emitted only 50 ms after the previous emit.
#[test]
fn middle_entries_are_throttled_to_one_per_50_ms() {
    assert!(!should_emit(500, 1000, Some(Duration::from_millis(0))));
    assert!(!should_emit(500, 1000, Some(Duration::from_millis(10))));
    assert!(!should_emit(500, 1000, Some(Duration::from_millis(49))));
    assert!(should_emit(500, 1000, Some(Duration::from_millis(50))));
    assert!(should_emit(500, 1000, Some(Duration::from_secs(2))));
}
