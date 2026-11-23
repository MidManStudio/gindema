// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-common.md, section "rng.rs"
// ============================================================================

//! A minimal SplitMix64 generator. Not cryptographically secure and not
//! trying to be — this is for seeded, reproducible *variation* (which
//! opcode numbering a build uses, which container slots are decoys, which
//! honeypot constants to use), not for anything whose secrecy matters. A
//! real key (see `gindema-container`'s own key generation, which goes
//! through `dixscript`'s own CSPRNG-backed key material instead of this)
//! is a different concern entirely. Originally written inside
//! `gindema-obfuscate`; moved here once `gindema-container` needed the
//! exact same thing rather than a second copy.

/// A SplitMix64 pseudorandom generator, seeded from a single `u64`.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Creates a generator seeded with `seed`. The same seed always
    /// produces the same sequence from this point on.
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next pseudorandom `u64` in the sequence.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value uniform-ish in `0..bound` (`bound` must be nonzero). Plain
    /// modulo, so not perfectly uniform for a non-power-of-two `bound` —
    /// an accepted bias for this generator's use (see the module's own
    /// doc comment); nothing here is a cryptographic shuffle.
    pub fn next_below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    /// `count` random bytes, four at a time from successive [`next_u64`]
    /// calls' low bytes. Used for non-secret filler content (e.g. a
    /// container's honeypot slots) — never for key or nonce material,
    /// which needs a real CSPRNG, not this.
    pub fn fill_bytes(&mut self, count: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(count);
        while out.len() < count {
            out.extend_from_slice(&self.next_u64().to_le_bytes());
        }
        out.truncate(count);
        out
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

    #[test]
    fn fill_bytes_returns_exactly_the_requested_length() {
        let mut rng = SplitMix64::new(3);
        assert_eq!(rng.fill_bytes(0).len(), 0);
        assert_eq!(rng.fill_bytes(5).len(), 5);
        assert_eq!(rng.fill_bytes(37).len(), 37);
    }

    #[test]
    fn fill_bytes_is_deterministic_for_the_same_seed() {
        let mut a = SplitMix64::new(99);
        let mut b = SplitMix64::new(99);
        assert_eq!(a.fill_bytes(20), b.fill_bytes(20));
    }
}
