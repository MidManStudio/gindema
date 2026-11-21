// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "watch.rs"
// ============================================================================

use gindema_common::{checksum, checksums_match};

use crate::state::GuardState;

/// Remembers the checksum of a byte region at the moment [`Watch::new`] was
/// called, so a later [`Watch::check`] can notice the region has changed
/// since then.
///
/// Holds only the expected checksum, not a copy of the data itself — the
/// caller owns the actual bytes and decides when and how often to re-check
/// them. See `docs/gindema-guard.md` for what kinds of regions this is
/// (and isn't) meant to watch.
#[derive(Debug, Clone, Copy)]
pub struct Watch {
    expected: u64,
}

impl Watch {
    /// Records `data`'s current checksum as the expected one.
    pub fn new(data: &[u8]) -> Self {
        Self {
            expected: checksum(data),
        }
    }

    /// Re-checksums `data` and compares it against what was recorded at
    /// construction. On a mismatch, pollutes `state` and returns `false`;
    /// on a match, returns `true` and leaves `state` exactly as it was
    /// (including, deliberately, staying polluted if something else
    /// already polluted it — this call never un-pollutes).
    pub fn check(&self, data: &[u8], state: &GuardState) -> bool {
        let matches = checksums_match(checksum(data), self.expected);
        if !matches {
            state.pollute();
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_data_keeps_the_state_clean() {
        let data = vec![1u8, 2, 3, 4, 5];
        let watch = Watch::new(&data);
        let state = GuardState::new();
        assert!(watch.check(&data, &state));
        assert!(!state.is_polluted());
    }

    #[test]
    fn changed_data_pollutes_the_state() {
        let mut data = vec![1u8, 2, 3, 4, 5];
        let watch = Watch::new(&data);
        data[2] = 99;
        let state = GuardState::new();
        assert!(!watch.check(&data, &state));
        assert!(state.is_polluted());
    }

    #[test]
    fn a_later_passing_check_does_not_clean_an_already_polluted_state() {
        let data = vec![9u8, 9, 9];
        let watch = Watch::new(&data);
        let state = GuardState::new();
        state.pollute(); // something else already tripped it
        assert!(watch.check(&data, &state), "this data genuinely matches");
        assert!(state.is_polluted(), "a passing check never un-pollutes");
    }
}
