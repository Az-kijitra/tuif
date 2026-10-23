//! TDD (unit-test-writer, 2026-09-27): tests for `shared::cancel::CancelRegistry`
//! (moved from `fs_ops/commands.rs`; SPEC-0200-0001 §3.4.1 / §3.4.14 A).
//!
//! Specs: SPEC-0300-0014 R-386 (shared mechanism), R-387-2 (a cancel that
//! arrives before registration is kept, MUST-58), R-387-3 / AC-358-3 (ids do
//! not interfere), AC-358-2. The `fs_ops` copy of the same observations lives
//! in `fs_ops/commands_tests.rs` and stays as is (only its `use` path moves).

use super::cancel::{CancelGuard, CancelRegistry};
use std::sync::atomic::Ordering;

fn raised(flag: &std::sync::Arc<std::sync::atomic::AtomicBool>) -> bool {
    flag.load(Ordering::Relaxed)
}

// R-387-2 / AC-358-2 / MUST-58: cancel BEFORE register is not lost.
#[test]
fn r387_2_cancel_before_register_is_kept_by_register() {
    let registry = CancelRegistry::default();
    registry.request_cancel("op-early");
    let flag = registry.register("op-early");
    assert!(
        raised(&flag),
        "register must hand out the pre-recorded cancellation"
    );
}

// R-387-2: a plain register starts un-canceled.
#[test]
fn r387_2_fresh_registration_is_not_canceled() {
    let registry = CancelRegistry::default();
    assert!(!raised(&registry.register("op-fresh")));
}

// R-387-2: cancel AFTER register raises the very flag the operation polls.
#[test]
fn r387_2_cancel_after_register_raises_the_registered_flag() {
    let registry = CancelRegistry::default();
    let flag = registry.register("op-a");
    registry.request_cancel("op-a");
    assert!(raised(&flag));
}

// R-387-2: registering twice for one id shares one flag (existing flag kept).
#[test]
fn r387_2_register_keeps_and_shares_the_existing_flag() {
    let registry = CancelRegistry::default();
    let first = registry.register("op-a");
    let second = registry.register("op-a");
    registry.request_cancel("op-a");
    assert!(
        raised(&first) && raised(&second),
        "both handles must observe the cancel"
    );
}

// R-387-3 / AC-358-3: canceling one id never affects another.
#[test]
fn r387_3_canceling_one_id_does_not_affect_another() {
    let registry = CancelRegistry::default();
    let a = registry.register("op-a");
    let b = registry.register("op-b");
    registry.request_cancel("op-a");
    assert!(raised(&a));
    assert!(!raised(&b), "op-b must keep running");
}

// R-387-3: an early cancel recorded for op-a is not inherited by op-b.
#[test]
fn r387_3_early_cancel_is_not_inherited_by_a_different_id() {
    let registry = CancelRegistry::default();
    registry.request_cancel("op-a");
    let b = registry.register("op-b");
    let a = registry.register("op-a");
    assert!(!raised(&b));
    assert!(raised(&a));
}

// Cleanup: after `unregister`, the same id starts fresh (no stale cancel).
#[test]
fn unregister_removes_the_entry_so_a_reused_id_starts_fresh() {
    let registry = CancelRegistry::default();
    let flag = registry.register("op-a");
    registry.request_cancel("op-a");
    assert!(raised(&flag));
    registry.unregister("op-a");
    let again = registry.register("op-a");
    assert!(
        !raised(&again),
        "stale cancellation must not leak into a new operation with the same id"
    );
}

#[test]
fn unregister_of_an_unknown_id_is_a_noop() {
    let registry = CancelRegistry::default();
    let other = registry.register("op-other");
    registry.unregister("never-registered");
    registry.request_cancel("op-other");
    assert!(raised(&other));
}

// The registry is shared across threads (Tauri State + async commands).
#[test]
fn registry_can_be_used_from_several_threads() {
    let registry = std::sync::Arc::new(CancelRegistry::default());
    let flag = registry.register("op-threads");
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let registry = registry.clone();
            std::thread::spawn(move || {
                let id = format!("op-{i}");
                let f = registry.register(&id);
                registry.request_cancel(&id);
                let ok = raised(&f);
                registry.unregister(&id);
                ok
            })
        })
        .collect();
    for h in handles {
        assert!(h.join().expect("thread"));
    }
    registry.request_cancel("op-threads");
    assert!(raised(&flag));
}

// OPTIONAL `CancelGuard` (SPEC-0200-0001 §3.4.1): drop unregisters (panic-safe
// cleanup). If code-generator does not adopt the guard, delete these tests.
#[test]
fn guard_registers_and_exposes_the_flag_and_keeps_an_early_cancel() {
    let registry = CancelRegistry::default();
    registry.request_cancel("op-g");
    let guard = CancelGuard::new(&registry, "op-g");
    assert!(
        raised(guard.flag()),
        "an early cancel must be visible through the guard (R-387-2)"
    );
}

#[test]
fn guard_drop_unregisters_the_operation() {
    let registry = CancelRegistry::default();
    {
        let guard = CancelGuard::new(&registry, "op-g");
        registry.request_cancel("op-g");
        assert!(raised(guard.flag()));
    }
    let again = registry.register("op-g");
    assert!(
        !raised(&again),
        "dropping the guard must remove the id's entry"
    );
}

#[test]
fn guard_drop_unregisters_even_when_the_operation_panics() {
    let registry = CancelRegistry::default();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = CancelGuard::new(&registry, "op-panic");
        registry.request_cancel("op-panic");
        panic!("simulated failure inside the operation");
    }));
    assert!(result.is_err());
    assert!(
        !raised(&registry.register("op-panic")),
        "the guard must clean up during unwinding"
    );
}
