//! `CancelRegistry`, shared by `fs_ops` and `archive` as ONE `State` instance
//! (SPEC-0200-0001 §3.4.1 / §3.4.14 A; SPEC-0300-0011 R-42/R-43/R-43-2,
//! SPEC-0300-0014 R-386, R-387-2, R-387-3). Moved here from
//! `fs_ops/commands.rs` so the "cancel before register is not lost" behavior
//! (MUST-58) has a single implementation.
//!
//! This is a plain lookup table, *not* a Storage Provider abstraction
//! (SPEC-0400-0001 §4.1): no trait, no `dyn`, no generic backend parameter.
//! Tests: `shared/cancel_tests.rs`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Per-`operation_id` cancellation flags (one Tauri `State` instance,
/// `.manage()`d in `lib.rs`).
#[derive(Default)]
pub struct CancelRegistry(pub(crate) Mutex<HashMap<String, Arc<AtomicBool>>>);

impl CancelRegistry {
    /// Registers `operation_id` and returns its cancellation flag. An existing
    /// flag (e.g. one a `request_cancel` that arrived earlier already raised,
    /// R-387-2) is kept and shared rather than replaced.
    pub fn register(&self, operation_id: &str) -> Arc<AtomicBool> {
        self.0
            .lock()
            .expect("cancel registry mutex poisoned")
            .entry(operation_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    }

    /// Removes `operation_id`'s entry once its operation has ended, so a cancel
    /// against a stale id is a harmless no-op rather than a leaked flag.
    pub fn unregister(&self, operation_id: &str) {
        self.0.lock().expect("cancel registry mutex poisoned").remove(operation_id);
    }

    /// R-43-2 / R-387-2 (MUST-58): raises the flag for `operation_id`. For an
    /// id that is not registered yet, a raised flag is pre-recorded and
    /// consumed by the next `register` of that id.
    pub fn request_cancel(&self, operation_id: &str) {
        self.0
            .lock()
            .expect("cancel registry mutex poisoned")
            .entry(operation_id.to_string())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .store(true, Ordering::Relaxed);
    }
}

/// RAII guard (SPEC-0200-0001 §3.4.1): registers on creation and
/// `unregister`s on drop, so a panic cannot leak the registration. Used by
/// `archive`.
pub struct CancelGuard<'a> {
    registry: &'a CancelRegistry,
    operation_id: String,
    flag: Arc<AtomicBool>,
}

impl<'a> CancelGuard<'a> {
    pub fn new(registry: &'a CancelRegistry, operation_id: &str) -> Self {
        let flag = registry.register(operation_id);
        CancelGuard { registry, operation_id: operation_id.to_string(), flag }
    }

    /// The operation's cancellation flag.
    pub fn flag(&self) -> &Arc<AtomicBool> {
        &self.flag
    }
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        self.registry.unregister(&self.operation_id);
    }
}
