// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "serialize.rs"
// ============================================================================

use crate::error::SerializeError;
use crate::opcode::{OpCode, OPCODE_COUNT};
use crate::opcode_map::OpcodeMap;
use crate::program::Program;
use crate::value::Value;

/// Magic bytes identifying a serialized gindema-vm program file.
const MAGIC: &[u8; 4] = b"GVMP";
/// The only format version this crate currently reads or writes.
const VERSION: u16 = 1;

const VALUE_TAG_INT: u8 = 0;
const VALUE_TAG_FLOAT: u8 = 1;
const VALUE_TAG_BOOL: u8 = 2;

/// A minimal cursor over a byte slice, used only by [`Program::from_bytes`].
/// Not a generalized binary-format library — this format has exactly one
/// reader and one writer, both in this file, and both change together.
struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SerializeError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(SerializeError::UnexpectedEof)?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or(SerializeError::UnexpectedEof)?;
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, SerializeError> {
        Ok(self.take(1)?[0])
    }

    fn u16_le(&mut self) -> Result<u16, SerializeError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .expect("take(2) returns exactly 2 bytes"),
        ))
    }

    fn u32_le(&mut self) -> Result<u32, SerializeError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .expect("take(4) returns exactly 4 bytes"),
        ))
    }

    fn u64_le(&mut self) -> Result<u64, SerializeError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .expect("take(8) returns exactly 8 bytes"),
        ))
    }

    fn i64_le(&mut self) -> Result<i64, SerializeError> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .expect("take(8) returns exactly 8 bytes"),
        ))
    }

    fn f64_le(&mut self) -> Result<f64, SerializeError> {
        Ok(f64::from_le_bytes(
            self.take(8)?
                .try_into()
                .expect("take(8) returns exactly 8 bytes"),
        ))
    }
}

impl Program {
    /// Serializes this program to a self-contained byte buffer: everything
    /// [`Program::from_bytes`] needs to reconstruct it exactly (bytecode,
    /// its exact real bit length, constants, local count, and the opcode
    /// table it was encoded with — see `docs/gindema-vm.md` for the exact
    /// layout).
    ///
    /// Little-endian throughout. Not a bit-packed format itself (unlike the
    /// instruction stream inside `code`) — there's no reason for the
    /// *container* around an already bit-packed stream to also be bit-level;
    /// byte alignment here just makes the format easier to read by eye in a
    /// hex dump.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.push(self.local_count);
        out.extend_from_slice(&self.code_bit_len.to_le_bytes());

        // The opcode table, as the one raw value each canonical opcode
        // encodes to, in `OpCode::ALL` order — enough on its own to
        // reconstruct the full `OpcodeMap` via `from_permutation`.
        for op in OpCode::ALL {
            out.push(self.opcode_map.encode(op));
        }

        out.extend_from_slice(&(self.constants.len() as u32).to_le_bytes());
        for value in &self.constants {
            match value {
                Value::Int(i) => {
                    out.push(VALUE_TAG_INT);
                    out.extend_from_slice(&i.to_le_bytes());
                }
                Value::Float(f) => {
                    out.push(VALUE_TAG_FLOAT);
                    out.extend_from_slice(&f.to_le_bytes());
                }
                Value::Bool(b) => {
                    out.push(VALUE_TAG_BOOL);
                    out.push(u8::from(*b));
                }
            }
        }

        out.extend_from_slice(&(self.code.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.code);
        out
    }

    /// Parses a program back from bytes written by [`Program::to_bytes`].
    ///
    /// # Errors
    /// See [`SerializeError`]. Every field is validated as it's read —
    /// there is no trusted/untrusted distinction made here; a hand-edited
    /// or corrupted file is handled the same way a genuinely malicious one
    /// would be, by returning an error rather than panicking or silently
    /// substituting a default.
    pub fn from_bytes(data: &[u8]) -> Result<Self, SerializeError> {
        let mut r = ByteReader::new(data);

        if r.take(MAGIC.len())? != MAGIC {
            return Err(SerializeError::InvalidMagic);
        }
        let version = r.u16_le()?;
        if version != VERSION {
            return Err(SerializeError::UnsupportedVersion(version));
        }

        let local_count = r.u8()?;
        let code_bit_len = r.u64_le()?;

        let mut raw_values = [0u8; OPCODE_COUNT];
        for slot in &mut raw_values {
            *slot = r.u8()?;
        }
        let opcode_map = OpcodeMap::from_permutation(raw_values)?;

        let const_count = r.u32_le()? as usize;
        let mut constants = Vec::with_capacity(const_count.min(1 << 20));
        for _ in 0..const_count {
            let tag = r.u8()?;
            let value = match tag {
                VALUE_TAG_INT => Value::Int(r.i64_le()?),
                VALUE_TAG_FLOAT => Value::Float(r.f64_le()?),
                VALUE_TAG_BOOL => Value::Bool(r.u8()? != 0),
                other => return Err(SerializeError::InvalidValueTag(other)),
            };
            constants.push(value);
        }

        let code_len = r.u32_le()? as usize;
        let code = r.take(code_len)?.to_vec();

        Ok(Self {
            code,
            code_bit_len,
            constants,
            local_count,
            opcode_map,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::ProgramBuilder;
    use crate::remap::remap_program;
    use crate::vm::Vm;

    #[test]
    fn round_trips_a_program_with_every_value_kind() {
        let mut b = ProgramBuilder::new(1);
        let i = b.add_const(Value::Int(-42)).unwrap();
        let f = b.add_const(Value::Float(3.5)).unwrap();
        let bl = b.add_const(Value::Bool(true)).unwrap();
        b.load_const(i).store_local(0);
        b.load_const(f);
        b.pop();
        b.load_const(bl);
        b.halt();
        let original = b.build().unwrap();

        let bytes = original.to_bytes();
        let restored = Program::from_bytes(&bytes).unwrap();
        assert_eq!(original, restored);

        let mut vm_a = Vm::new();
        let mut vm_b = Vm::new();
        assert_eq!(vm_a.run(&original).unwrap(), vm_b.run(&restored).unwrap());
    }

    #[test]
    fn round_trips_a_remapped_programs_opcode_table_too() {
        let mut raw_values = [0u8; OPCODE_COUNT];
        for (i, v) in raw_values.iter_mut().enumerate() {
            *v = (63 - i * 3) as u8;
        }
        let map = OpcodeMap::from_permutation(raw_values).unwrap();

        let mut b = ProgramBuilder::new(0);
        let two = b.add_const(Value::Int(2)).unwrap();
        b.load_const(two).neg().halt();
        let original = b.build().unwrap();
        let remapped = remap_program(&original, &map).unwrap();

        let restored = Program::from_bytes(&remapped.to_bytes()).unwrap();
        assert_eq!(remapped, restored);
        assert_eq!(restored.opcode_map(), &map);
    }

    #[test]
    fn wrong_magic_is_rejected() {
        let mut bytes = ProgramBuilder::new(0).halt().build().unwrap().to_bytes();
        bytes[0] = b'X';
        assert_eq!(
            Program::from_bytes(&bytes),
            Err(SerializeError::InvalidMagic)
        );
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let mut bytes = ProgramBuilder::new(0).halt().build().unwrap().to_bytes();
        bytes[4] = 0xFF;
        bytes[5] = 0xFF;
        assert_eq!(
            Program::from_bytes(&bytes),
            Err(SerializeError::UnsupportedVersion(0xFFFF))
        );
    }

    #[test]
    fn truncated_data_is_rejected_not_panicked() {
        let bytes = ProgramBuilder::new(0).halt().build().unwrap().to_bytes();
        for cut in 0..bytes.len() {
            assert_eq!(
                Program::from_bytes(&bytes[..cut]),
                Err(SerializeError::UnexpectedEof)
            );
        }
    }

    #[test]
    fn invalid_value_tag_is_rejected() {
        let mut b = ProgramBuilder::new(0);
        b.add_const(Value::Int(0)).unwrap();
        b.halt();
        let mut bytes = b.build().unwrap().to_bytes();
        // Byte layout: 4 magic + 2 version + 1 local_count + 8 code_bit_len
        // + OPCODE_COUNT opcode table + 4 constant count = the tag byte of
        // the first constant comes right after that.
        let tag_offset = 4 + 2 + 1 + 8 + OPCODE_COUNT + 4;
        bytes[tag_offset] = 99;
        assert_eq!(
            Program::from_bytes(&bytes),
            Err(SerializeError::InvalidValueTag(99))
        );
    }

    #[test]
    fn duplicate_opcode_table_entries_are_rejected() {
        let mut bytes = ProgramBuilder::new(0).halt().build().unwrap().to_bytes();
        let table_offset = 4 + 2 + 1 + 8;
        bytes[table_offset + 1] = bytes[table_offset];
        assert!(matches!(
            Program::from_bytes(&bytes),
            Err(SerializeError::OpcodeMap(_))
        ));
    }
}
