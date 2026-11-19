// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-bits.md, section "reader.rs"
// ============================================================================

use crate::error::BitsError;

/// Reads individual bits and arbitrary-width (1..=64 bit) unsigned fields back out
/// of a byte slice, most-significant-bit first within each byte — the inverse of
/// [`crate::BitWriter`].
///
/// Tracks an absolute bit position rather than wrapping a generic `Read`, so it
/// can [`BitReader::seek_to_bit`] to an arbitrary position. `gindema-vm` needs
/// this for jump/branch instructions: a jump operand is the absolute bit offset
/// of its target instruction, and the interpreter seeks there directly instead
/// of re-decoding every instruction in between.
#[derive(Debug, Clone, Copy)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_pos: u64,
}

impl<'a> BitReader<'a> {
    /// Creates a reader positioned at the start of `bytes`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    /// The stream's total length, in bits.
    pub fn bit_len(&self) -> u64 {
        self.bytes.len() as u64 * 8
    }

    /// The current absolute read position, in bits from the start of the stream.
    pub fn bit_pos(&self) -> u64 {
        self.bit_pos
    }

    /// How many bits remain unread.
    pub fn bits_remaining(&self) -> u64 {
        self.bit_len() - self.bit_pos
    }

    /// Reads a single bit.
    ///
    /// # Errors
    /// [`BitsError::EndOfStream`] if the stream is already exhausted.
    pub fn read_bit(&mut self) -> Result<bool, BitsError> {
        if self.bit_pos >= self.bit_len() {
            return Err(BitsError::EndOfStream);
        }
        let byte_idx = (self.bit_pos / 8) as usize;
        let bit_idx = 7 - (self.bit_pos % 8) as u32;
        let bit = (self.bytes[byte_idx] >> bit_idx) & 1 == 1;
        self.bit_pos += 1;
        Ok(bit)
    }

    /// Reads `width` bits as an unsigned value, most-significant bit first —
    /// the exact inverse of [`crate::BitWriter::write_bits`] with the same width.
    ///
    /// # Errors
    /// - [`BitsError::InvalidWidth`] if `width` is 0 or greater than 64.
    /// - [`BitsError::EndOfStream`] if fewer than `width` bits remain.
    ///
    /// On an `EndOfStream` error, the reader's position is left exactly where it
    /// stopped (partway through the field) rather than rewound — callers that
    /// need to retry from the start should track their own position via
    /// [`BitReader::bit_pos`]/[`BitReader::seek_to_bit`] first.
    pub fn read_bits(&mut self, width: u32) -> Result<u64, BitsError> {
        if width == 0 || width > 64 {
            return Err(BitsError::InvalidWidth(width));
        }
        let mut value: u64 = 0;
        for _ in 0..width {
            value = (value << 1) | u64::from(self.read_bit()?);
        }
        Ok(value)
    }

    /// Advances to the next byte boundary, discarding any skipped bits. A no-op
    /// if already byte-aligned. Does not check whether the resulting position is
    /// still in range — a subsequent read past the end still reports
    /// [`BitsError::EndOfStream`] rather than this call failing silently early.
    pub fn byte_align(&mut self) {
        let rem = self.bit_pos % 8;
        if rem != 0 {
            self.bit_pos += 8 - rem;
        }
    }

    /// Jumps directly to an absolute bit position.
    ///
    /// # Errors
    /// [`BitsError::SeekOutOfRange`] if `pos` is past the end of the stream.
    /// Seeking exactly to `bit_len()` is allowed (an empty read from there then
    /// reports `EndOfStream`, same as reaching that position by reading).
    pub fn seek_to_bit(&mut self, pos: u64) -> Result<(), BitsError> {
        let len = self.bit_len();
        if pos > len {
            return Err(BitsError::SeekOutOfRange { pos, len });
        }
        self.bit_pos = pos;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::BitWriter;

    #[test]
    fn round_trips_a_mix_of_widths() {
        let mut w = BitWriter::new();
        w.write_bits(0b1, 1).unwrap();
        w.write_bits(0b101, 3).unwrap();
        w.write_bits(0xAB, 8).unwrap();
        w.write_bits(0x1234, 16).unwrap();
        w.write_bits(0xDEAD_BEEF, 32).unwrap();
        w.write_bits(u64::MAX, 64).unwrap();
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_bits(1).unwrap(), 0b1);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        assert_eq!(r.read_bits(8).unwrap(), 0xAB);
        assert_eq!(r.read_bits(16).unwrap(), 0x1234);
        assert_eq!(r.read_bits(32).unwrap(), 0xDEAD_BEEF);
        assert_eq!(r.read_bits(64).unwrap(), u64::MAX);
    }

    #[test]
    fn read_bits_rejects_zero_and_over_64_width() {
        let bytes = [0u8; 4];
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_bits(0), Err(BitsError::InvalidWidth(0)));
        assert_eq!(r.read_bits(65), Err(BitsError::InvalidWidth(65)));
    }

    #[test]
    fn reading_past_the_end_reports_end_of_stream() {
        let bytes = [0xFFu8; 1];
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_bits(8).unwrap(), 0xFF);
        assert_eq!(r.read_bit(), Err(BitsError::EndOfStream));
        assert_eq!(r.read_bits(1), Err(BitsError::EndOfStream));
    }

    #[test]
    fn byte_align_skips_to_the_next_byte_boundary() {
        let bytes = [0b1010_1010, 0b1111_0000];
        let mut r = BitReader::new(&bytes);
        r.read_bits(3).unwrap();
        assert_eq!(r.bit_pos(), 3);
        r.byte_align();
        assert_eq!(r.bit_pos(), 8);
        assert_eq!(r.read_bits(8).unwrap(), 0b1111_0000);
    }

    #[test]
    fn seek_to_bit_jumps_directly_to_a_target() {
        let mut w = BitWriter::new();
        w.write_bits(0xFF, 8).unwrap();
        let target = w.bit_len();
        w.write_bits(0x7, 3).unwrap();
        w.write_bits(0x2A, 8).unwrap();
        let bytes = w.into_bytes();

        let mut r = BitReader::new(&bytes);
        r.seek_to_bit(target).unwrap();
        assert_eq!(r.read_bits(3).unwrap(), 0x7);
        assert_eq!(r.read_bits(8).unwrap(), 0x2A);
    }

    #[test]
    fn seek_past_the_end_is_rejected() {
        let bytes = [0u8; 2];
        let mut r = BitReader::new(&bytes);
        assert_eq!(
            r.seek_to_bit(17),
            Err(BitsError::SeekOutOfRange { pos: 17, len: 16 })
        );
        // Seeking exactly to the end is fine; reading from there is not.
        assert!(r.seek_to_bit(16).is_ok());
        assert_eq!(r.read_bit(), Err(BitsError::EndOfStream));
    }

    #[test]
    fn bits_remaining_tracks_position() {
        let bytes = [0u8; 2];
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.bits_remaining(), 16);
        r.read_bits(5).unwrap();
        assert_eq!(r.bits_remaining(), 11);
    }
}
