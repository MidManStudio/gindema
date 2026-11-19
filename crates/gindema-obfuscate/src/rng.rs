// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "rng.rs"
// ============================================================================

//! A minimal SplitMix64 generator — the same choice `mirage-train` made for
//! its own seeded shuffling (see `docs/mirage-train.md`). Not
//! cryptographically secure and not trying to be: this is for build-to-build
//! *variation*, not secrecy strong enough to resist a determined attacker who
//! can brute-force or statistically attack a 64-bit permutation space. Pulling
//! in a real `rand`-ecosystem crate for one small shuffle felt like the wrong
//! trade for a dependency this crate doesn't otherwise need.

/// A SplitMix64 pseudorandom generator, seeded from a single `u64`.
#[derive(Debug, Clone)]
pub(crate) struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value uniform-ish in `0..bound` (`bound` must be nonzero). Plain
    /// modulo, so not perfectly uniform for a non-power-of-two `bound` — an
    /// accepted bias for this crate's use (shuffling a 64-entry opcode table
    /// for build-to-build variation, not a cryptographic shuffle).
    pub(crate) fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_always_produces_the_same_sequence() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..8 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = SplitMix64::new(1);
        let mut b = SplitMix64::new(2);
        let seq_a: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let seq_b: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_ne!(seq_a, seq_b);
    }

    #[test]
    fn next_below_stays_within_bound() {
        let mut rng = SplitMix64::new(7);
        for _ in 0..200 {
            assert!(rng.next_below(64) < 64);
        }
    }
}
