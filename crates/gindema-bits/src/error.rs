// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-bits.md, section "error.rs"
// ============================================================================

use std::fmt;

/// Everything that can go wrong reading or writing a bit-level stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitsError {
    /// A bit width was 0, or more than 64 (the widest value [`crate::BitReader`]
    /// and [`crate::BitWriter`] move at a time).
    InvalidWidth(u32),
    /// [`crate::BitWriter::write_bits`] was given a value with bits set above
    /// the requested width — it would silently truncate instead of encoding
    /// what was asked for.
    ValueTooWide {
        /// The width that was requested.
        width: u32,
    },
    /// A read ran past the end of the underlying byte slice.
    EndOfStream,
    /// [`crate::BitReader::seek_to_bit`] was given a position past the end of
    /// the underlying byte slice.
    SeekOutOfRange {
        /// The position that was requested.
        pos: u64,
        /// The stream's total length in bits.
        len: u64,
    },
}

impl fmt::Display for BitsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWidth(w) => {
                write!(f, "bit width {w} is invalid (must be 1..=64)")
            }
            Self::ValueTooWide { width } => {
                write!(f, "value does not fit in {width} bits")
            }
            Self::EndOfStream => write!(f, "read past the end of the bit stream"),
            Self::SeekOutOfRange { pos, len } => {
                write!(
                    f,
                    "seek to bit {pos} is out of range for a {len}-bit stream"
                )
            }
        }
    }
}

impl std::error::Error for BitsError {}
