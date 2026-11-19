// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "guarded.rs"
// ============================================================================

use crate::state::GuardState;

/// A value that silently reads as a decoy instead of its real contents once
/// a shared [`GuardState`] has been polluted — "returns plausible garbage
/// instead of crashing," applied to one specific value rather than the
/// whole program.
///
/// Both the real value and the decoy are set once, at construction, by
/// whoever creates the `Guarded`. Choosing a decoy that's actually
/// plausible for the surrounding code (a believable score, a believable
/// position, not an obviously-wrong sentinel) is the caller's call to
/// make — this type only handles the switch between the two.
#[derive(Debug, Clone, Copy)]
pub struct Guarded<T> {
    real: T,
    decoy: T,
}

impl<T: Copy> Guarded<T> {
    /// Wraps `real`, falling back to `decoy` once `state` is polluted.
    pub fn new(real: T, decoy: T) -> Self {
        Self { real, decoy }
    }

    /// Returns the real value if `state` is clean, the decoy if polluted.
    pub fn read(&self, state: &GuardState) -> T {
        if state.is_polluted() {
            self.decoy
        } else {
            self.real
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_real_value_while_clean() {
        let guarded = Guarded::new(100, -1);
        let state = GuardState::new();
        assert_eq!(guarded.read(&state), 100);
    }

    #[test]
    fn switches_to_the_decoy_once_polluted() {
        let guarded = Guarded::new(100, -1);
        let state = GuardState::new();
        state.pollute();
        assert_eq!(guarded.read(&state), -1);
    }

    #[test]
    fn several_guarded_values_sharing_one_state_all_switch_together() {
        let state = GuardState::new();
        let health = Guarded::new(100u32, 100u32); // a decoy that's the same as real on purpose below
        let score = Guarded::new(9001i64, 0i64);

        assert_eq!(health.read(&state), 100);
        assert_eq!(score.read(&state), 9001);

        state.pollute();

        assert_eq!(
            health.read(&state),
            100,
            "its decoy happens to equal its real value"
        );
        assert_eq!(
            score.read(&state),
            0,
            "its decoy is visibly different once polluted"
        );
    }
}
