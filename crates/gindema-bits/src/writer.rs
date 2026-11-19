// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-bits.md, section "writer.rs"
// ============================================================================

use crate::error::BitsError;

/// Packs individual bits and arbitrary-width (1..=64 bit) unsigned fields into a
/// byte buffer, most-significant-bit first within each byte.
///
/// `gindema-vm` uses this to encode its bytecode stream: a 6-bit opcode field
/// followed by whatever operand width that opcode needs, with no byte-alignment
/// between instructions. [`BitWriter::bit_len`] reports the current bit position,
/// which a compiler uses to record jump targets before the destination
/// instruction has been written yet (back-patching is the caller's job; this
/// type only tracks position).
#[derive(Debug, Default, Clone)]
pub struct BitWriter {
    bytes: Vec<u8>,
    /// Bits already placed in the partially-filled next byte, 0..=7.
    cur: u8,
    cur_bits: u8,
}

impl BitWriter {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes a single bit.
    pub fn write_bit(&mut self, bit: bool) {
        self.cur = (self.cur << 1) | u8::from(bit);
        self.cur_bits += 1;
        if self.cur_bits == 8 {
            self.bytes.push(self.cur);
            self.cur = 0;
            self.cur_bits = 0;
        }
    }

    /// Writes the low `width` bits of `value`, most-significant bit first.
    ///
    /// # Errors
    /// - [`BitsError::InvalidWidth`] if `width` is 0 or greater than 64.
    /// - [`BitsError::ValueTooWide`] if `value` has any bit set above `width`
    ///   (silently truncating would encode something other than what was asked
    ///   for, so this is rejected rather than masked).
    pub fn write_bits(&mut self, value: u64, width: u32) -> Result<(), BitsError> {
        if width == 0 || width > 64 {
            return Err(BitsError::InvalidWidth(width));
        }
        if width < 64 && (value >> width) != 0 {
            return Err(BitsError::ValueTooWide { width });
        }
        for i in (0..width).rev() {
            self.write_bit((value >> i) & 1 == 1);
        }
        Ok(())
    }

    /// Pads with `0` bits up to the next byte boundary. A no-op if already
    /// byte-aligned.
    pub fn byte_align(&mut self) {
        while self.cur_bits != 0 {
            self.write_bit(false);
        }
    }

    /// The current length of the stream, in bits (including the partially-filled
    /// trailing byte, if any).
    pub fn bit_len(&self) -> u64 {
        self.bytes.len() as u64 * 8 + u64::from(self.cur_bits)
    }

    /// Consumes the writer, byte-aligning it first, and returns the packed bytes.
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.byte_align();
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_bits_rejects_zero_and_over_64_width() {
        let mut w = BitWriter::new();
        assert_eq!(w.write_bits(0, 0), Err(BitsError::InvalidWidth(0)));
        assert_eq!(w.write_bits(0, 65), Err(BitsError::InvalidWidth(65)));
    }

    #[test]
    fn write_bits_rejects_a_value_that_does_not_fit_the_width() {
        let mut w = BitWriter::new();
        assert_eq!(
            w.write_bits(0b1000, 3),
            Err(BitsError::ValueTooWide { width: 3 })
        );
        // Exactly fits: no error.
        assert_eq!(w.write_bits(0b111, 3), Ok(()));
    }

    #[test]
    fn byte_align_pads_with_zero_bits() {
        let mut w = BitWriter::new();
        w.write_bits(0b101, 3).unwrap();
        assert_eq!(w.bit_len(), 3);
        w.byte_align();
        assert_eq!(w.bit_len(), 8);
        assert_eq!(w.into_bytes(), vec![0b1010_0000]);
    }

    #[test]
    fn byte_align_on_an_already_aligned_writer_is_a_no_op() {
        let mut w = BitWriter::new();
        w.write_bits(0xAB, 8).unwrap();
        assert_eq!(w.bit_len(), 8);
        w.byte_align();
        assert_eq!(w.bit_len(), 8);
    }

    #[test]
    fn bit_len_tracks_position_before_byte_alignment() {
        let mut w = BitWriter::new();
        assert_eq!(w.bit_len(), 0);
        w.write_bits(0b11, 2).unwrap();
        assert_eq!(w.bit_len(), 2);
        w.write_bits(0xFFFF, 16).unwrap();
        assert_eq!(w.bit_len(), 18);
    }

    #[test]
    fn into_bytes_byte_aligns_before_returning() {
        let mut w = BitWriter::new();
        w.write_bit(true);
        let bytes = w.into_bytes();
        assert_eq!(bytes, vec![0b1000_0000]);
    }
}
