// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "checksum.rs"
// ============================================================================

/// FNV-1a's 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a's 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// A fast, non-cryptographic checksum over a byte slice (FNV-1a, 64-bit).
///
/// This is an integrity check against accidental or casual corruption and
/// patching, not a security primitive: FNV-1a has no resistance to a
/// deliberate, informed attacker constructing a different byte sequence
/// with the same checksum (a second-preimage attack) the way a
/// cryptographic hash would. For gindema-guard's purpose — noticing that a
/// watched region of memory changed from what it was when watching started
/// — that's enough; see `docs/gindema-guard.md` for the fuller reasoning
/// and what a stronger threat model would need instead.
pub fn checksum(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Compares two checksums without an early-exit branch on the first
/// differing bit, for whatever small amount of timing-side-channel
/// resistance that's worth. Not a rigorous constant-time guarantee — LLVM
/// is free to optimize this however it likes, and nothing here has been
/// verified against the actual compiled output the way a real
/// constant-time primitive would need to be. Used by `watch.rs` instead of
/// a plain `==` mainly so that choice is visible and documented rather than
/// silently defaulted into.
pub(crate) fn checksums_match(a: u64, b: u64) -> bool {
    (a ^ b) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bytes_always_give_the_same_checksum() {
        let data = b"the quick brown fox";
        assert_eq!(checksum(data), checksum(data));
    }

    #[test]
    fn different_bytes_give_a_different_checksum() {
        assert_ne!(checksum(b"hello"), checksum(b"hellp"));
        assert_ne!(checksum(b""), checksum(b"\0"));
    }

    #[test]
    fn empty_input_does_not_panic_and_is_the_offset_basis() {
        assert_eq!(checksum(b""), FNV_OFFSET_BASIS);
    }

    #[test]
    fn checksums_match_agrees_with_equality() {
        assert!(checksums_match(42, 42));
        assert!(!checksums_match(42, 43));
    }
}
