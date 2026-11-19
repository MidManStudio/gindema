// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "opcode_map.rs"
// ============================================================================

use std::fmt;

use crate::opcode::{OpCode, OPCODE_COUNT};

/// Everything that can go wrong building an [`OpcodeMap`] from a caller-supplied
/// permutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpcodeMapError {
    /// A raw value was 64 or greater — the opcode field is 6 bits wide.
    RawValueOutOfRange(u8),
    /// The same raw value was assigned to two different opcodes.
    DuplicateRawValue(u8),
}

impl fmt::Display for OpcodeMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RawValueOutOfRange(v) => write!(f, "raw opcode value {v} does not fit in 6 bits"),
            Self::DuplicateRawValue(v) => write!(f, "raw opcode value {v} was assigned twice"),
        }
    }
}

impl std::error::Error for OpcodeMapError {}

/// Maps the raw 6-bit opcode value written to the wire to and from the
/// logical [`OpCode`] it means for one particular [`crate::Program`].
///
/// [`OpcodeMap::identity`] — what [`crate::ProgramBuilder::build`] always
/// uses — maps each opcode to its own [`OpCode::to_u8`] value, exactly as
/// `opcode.rs` defines it. A non-identity map, built with
/// [`OpcodeMap::from_permutation`], lets two builds of the same program
/// disagree about which raw byte means `Add` versus `Jump` versus anything
/// else, without changing the program's logic, its operand widths, or any
/// bit offset within it — see [`crate::remap_program`] for why that last
/// part makes remapping a pure, lossless transform. Generating the actual
/// permutation (randomly, per build) is `gindema-obfuscate`'s job, not
/// this crate's; this type only validates and applies whatever permutation
/// it's given.
#[derive(Debug, Clone, PartialEq)]
pub struct OpcodeMap {
    decode: [Option<OpCode>; 64],
    /// Indexed by each opcode's own canonical `to_u8()` value — stable
    /// regardless of what this map's permutation actually is, so encoding
    /// doesn't need to search.
    encode: [u8; OPCODE_COUNT],
}

impl OpcodeMap {
    /// The identity map: every opcode encodes to its own canonical value.
    pub fn identity() -> Self {
        let mut decode = [None; 64];
        let mut encode = [0u8; OPCODE_COUNT];
        for op in OpCode::ALL {
            let raw = op.to_u8();
            decode[raw as usize] = Some(op);
            encode[raw as usize] = raw;
        }
        Self { decode, encode }
    }

    /// Builds a map from a caller-supplied permutation: `raw_values[i]` is
    /// the raw 6-bit value that should mean `OpCode::ALL[i]`.
    ///
    /// # Errors
    /// [`OpcodeMapError::RawValueOutOfRange`] if any value is 64 or more.
    /// [`OpcodeMapError::DuplicateRawValue`] if two opcodes are assigned the
    /// same raw value.
    pub fn from_permutation(raw_values: [u8; OPCODE_COUNT]) -> Result<Self, OpcodeMapError> {
        let mut decode = [None; 64];
        let mut encode = [0u8; OPCODE_COUNT];
        for (op, &raw) in OpCode::ALL.iter().zip(raw_values.iter()) {
            if raw >= 64 {
                return Err(OpcodeMapError::RawValueOutOfRange(raw));
            }
            if decode[raw as usize].is_some() {
                return Err(OpcodeMapError::DuplicateRawValue(raw));
            }
            decode[raw as usize] = Some(*op);
            encode[op.to_u8() as usize] = raw;
        }
        Ok(Self { decode, encode })
    }

    /// Decodes a raw 6-bit value into the opcode it means under this map, or
    /// `None` if this map leaves that value unassigned.
    pub fn decode(&self, raw: u8) -> Option<OpCode> {
        self.decode.get(raw as usize).copied().flatten()
    }

    /// The raw 6-bit value `opcode` encodes to under this map.
    pub fn encode(&self, opcode: OpCode) -> u8 {
        self.encode[opcode.to_u8() as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trips_every_opcode_through_its_own_value() {
        let map = OpcodeMap::identity();
        for op in OpCode::ALL {
            let raw = map.encode(op);
            assert_eq!(raw, op.to_u8());
            assert_eq!(map.decode(raw), Some(op));
        }
    }

    #[test]
    fn a_scrambled_permutation_round_trips_too() {
        // Reverse the usual 0..=16 assignment and scatter it upward, just to
        // be sure nothing secretly assumes the identity ordering.
        let mut raw_values = [0u8; OPCODE_COUNT];
        for (i, v) in raw_values.iter_mut().enumerate() {
            *v = (63 - i) as u8;
        }
        let map = OpcodeMap::from_permutation(raw_values).unwrap();
        for (op, &raw) in OpCode::ALL.iter().zip(raw_values.iter()) {
            assert_eq!(map.encode(*op), raw);
            assert_eq!(map.decode(raw), Some(*op));
        }
    }

    #[test]
    fn out_of_range_raw_value_is_rejected() {
        let mut raw_values = [0u8; OPCODE_COUNT];
        for (i, v) in raw_values.iter_mut().enumerate() {
            *v = i as u8;
        }
        raw_values[0] = 64;
        assert_eq!(
            OpcodeMap::from_permutation(raw_values),
            Err(OpcodeMapError::RawValueOutOfRange(64))
        );
    }

    #[test]
    fn duplicate_raw_value_is_rejected() {
        let mut raw_values = [0u8; OPCODE_COUNT];
        for (i, v) in raw_values.iter_mut().enumerate() {
            *v = i as u8;
        }
        raw_values[1] = raw_values[0];
        assert_eq!(
            OpcodeMap::from_permutation(raw_values),
            Err(OpcodeMapError::DuplicateRawValue(raw_values[0]))
        );
    }

    #[test]
    fn unassigned_raw_values_decode_to_none() {
        let map = OpcodeMap::identity();
        assert_eq!(map.decode(63), None);
    }
}
