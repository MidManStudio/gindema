// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "state.rs"
// ============================================================================

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether tampering has been detected — shared by every [`crate::Watch`]
/// and [`crate::Guarded`] that watches the same protected region (or, for
/// a coarser blast radius, a whole session).
///
/// Deliberately one-directional: [`GuardState::pollute`] has no
/// counterpart to clean the state back up. Once *any* watcher trips,
/// everything sharing this `GuardState` stays polluted for as long as the
/// state lives — a single false positive is cheaper to live with than
/// giving an attacker who found a way to trip and then un-trip detection a
/// way to make a tampered run look clean again.
///
/// Plain `AtomicBool` under the hood rather than an `unsafe`-erased cell:
/// this is one bit of shared mutable state, which safe Rust already does
/// without needing anything from `gindema-ptr` — see
/// `docs/gindema-guard.md`'s "Known limits" for why that crate isn't used
/// here (yet).
#[derive(Debug, Default)]
pub struct GuardState {
    polluted: AtomicBool,
}

impl GuardState {
    /// A fresh, clean state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this state has been polluted by any watcher that shares it.
    pub fn is_polluted(&self) -> bool {
        self.polluted.load(Ordering::SeqCst)
    }

    /// Marks this state polluted. Idempotent, and permanent for this
    /// `GuardState`'s lifetime — see the type's own docs for why there's no
    /// way back.
    pub fn pollute(&self) {
        self.polluted.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_clean() {
        let state = GuardState::new();
        assert!(!state.is_polluted());
    }

    #[test]
    fn pollute_is_visible_immediately_and_stays_set() {
        let state = GuardState::new();
        state.pollute();
        assert!(state.is_polluted());
        // Calling it again changes nothing observable.
        state.pollute();
        assert!(state.is_polluted());
    }
}
