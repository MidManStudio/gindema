// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-common.md, section "checksum.rs"
// ============================================================================

/// FNV-1a's 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a's 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// A fast, non-cryptographic checksum over a byte slice (FNV-1a, 64-bit).
///
/// This is an integrity check against accidental or casual corruption, not
/// a security primitive: FNV-1a has no resistance to a deliberate,
/// informed attacker constructing a different byte sequence with the same
/// checksum. Originally written for `gindema-guard`'s tamper-detection
/// `Watch`, moved here once it became clear other crates (asset-container
/// integrity checks, program-file sanity checks) would plausibly want the
/// same thing rather than each rolling their own.
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
/// verified against actual compiled output. Exists mainly so that choice
/// is visible and documented at call sites rather than a plain `==`
/// silently defaulted into.
pub fn checksums_match(a: u64, b: u64) -> bool {
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
