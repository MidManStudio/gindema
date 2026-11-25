// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "decode.rs"
// ============================================================================

//! Decodes an already-built [`Program`] back into a structured instruction
//! list, rather than the raw bit stream [`Program::code_bytes`] exposes.
//! `gindema-obfuscate`'s control-flow flattening and state-dependent
//! decoding passes are both built on this -- flattening needs to see basic
//! block boundaries, which means seeing every instruction's own bit offset
//! and every jump's resolved target; the fluff pass needs the same thing to
//! compute new bit offsets once junk bits are interleaved. Closes part of
//! `docs/gindema-vm.md`'s former "no disassembler" known limit, though this
//! produces a structured list for another pass to consume, not human-
//! readable text -- a real disassembler would still need to print it.

use std::fmt;

use gindema_bits::BitReader;

use crate::error::VmError;
use crate::fluff::fluff_len_at;
use crate::opcode::{OpCode, CONST_INDEX_WIDTH, JUMP_TARGET_WIDTH, LOCAL_SLOT_WIDTH, OPCODE_WIDTH};
use crate::program::Program;

/// An instruction's already-interpreted operand. `None` for the twelve
/// opcodes that take none; see [`OpCode::operand_width`] for which is
/// which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    /// No operand.
    None,
    /// [`OpCode::LoadConst`]'s constant-pool index.
    ConstIndex(u16),
    /// [`OpCode::LoadLocal`]/[`OpCode::StoreLocal`]'s local-slot index.
    LocalSlot(u8),
    /// [`OpCode::Jump`]/[`OpCode::JumpIfFalse`]'s target -- an absolute bit
    /// offset into the *same* program's stream, exactly as encoded (if the
    /// program carries fluff, this is the target's fluff-start offset, the
    /// same value [`DecodedInstr::bit_offset`] reports for the instruction
    /// actually sitting there -- see [`decode_program`]'s docs).
    JumpTarget(u64),
}

/// One decoded instruction: its opcode, its operand, and the bit range it
/// occupies in the program's stream (`bit_offset .. bit_offset + bit_len`).
///
/// `bit_offset` is captured *before* any fluff preceding this instruction is
/// skipped (see [`decode_program`]), so it always equals the value a
/// `Jump`/`JumpIfFalse` targeting this instruction actually encodes --
/// fluff-aware callers (like `gindema-obfuscate`'s flattening pass) can
/// treat `bit_offset` as "this instruction's address" without caring
/// whether fluff is present at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedInstr {
    /// This instruction's opcode.
    pub opcode: OpCode,
    /// This instruction's operand, if any.
    pub operand: Operand,
    /// The bit offset this instruction's decoding started at -- see the
    /// struct-level docs above for exactly what that means when fluff is
    /// present.
    pub bit_offset: u64,
    /// The total number of bits this instruction occupies, fluff included.
    /// `bit_offset + bit_len` is always another instruction's `bit_offset`,
    /// or exactly the program's [`Program::code_bit_len`] if this is the
    /// last instruction in the stream.
    pub bit_len: u64,
}

/// Decodes `program`'s bytecode stream into a flat list of
/// [`DecodedInstr`]s, front to back, up to exactly [`Program::code_bit_len`]
/// bits -- the same exact-length walk [`crate::remap_program`] uses, for the
/// same reason (see that function's docs): this never follows control flow,
/// so it has no way to tell a reachable `Halt` from dead code's own one, and
/// stopping at the first `Halt` would silently truncate a program containing
/// a `gindema-obfuscate` honeypot.
///
/// Transparently skips fluff if `program.fluff_seed()` is `Some` (see
/// `docs/gindema-vm.md`'s `fluff.rs` section) -- the returned instructions
/// and their `bit_offset`s are identical either way; fluff is invisible to
/// every caller of this function.
///
/// # Errors
/// [`VmError::InvalidOpcode`] if a raw opcode byte doesn't decode under
/// `program.opcode_map()`. Propagates [`VmError::Bits`] if the stream is
/// truncated.
pub fn decode_program(program: &Program) -> Result<Vec<DecodedInstr>, VmError> {
    let mut reader = BitReader::new(program.code_bytes());
    let end_bit = program.code_bit_len();
    let mut out = Vec::new();

    while reader.bit_pos() < end_bit {
        let bit_offset = reader.bit_pos();
        if let Some(seed) = program.fluff_seed() {
            let flen = fluff_len_at(seed, bit_offset);
            if flen > 0 {
                reader.read_bits(flen)?;
            }
        }

        let raw = reader.read_bits(OPCODE_WIDTH)? as u8;
        let opcode = program
            .opcode_map()
            .decode(raw)
            .ok_or(VmError::InvalidOpcode(raw))?;

        let operand = match opcode {
            OpCode::LoadConst => Operand::ConstIndex(reader.read_bits(CONST_INDEX_WIDTH)? as u16),
            OpCode::LoadLocal | OpCode::StoreLocal => {
                Operand::LocalSlot(reader.read_bits(LOCAL_SLOT_WIDTH)? as u8)
            }
            OpCode::Jump | OpCode::JumpIfFalse => {
                Operand::JumpTarget(reader.read_bits(JUMP_TARGET_WIDTH)?)
            }
            _ => Operand::None,
        };

        out.push(DecodedInstr {
            opcode,
            operand,
            bit_offset,
            bit_len: reader.bit_pos() - bit_offset,
        });
    }

    Ok(out)
}

/// Resolves a `Jump`/`JumpIfFalse` target (an absolute bit offset, as
/// [`Operand::JumpTarget`] carries it) against `instrs` (which must be
/// exactly what [`decode_program`] returned for the same program, in the
/// same order): `Some(i)` if `target` is `instrs[i].bit_offset`, or `None`
/// if `target` is exactly `end_bit` -- the "jump past the very last
/// instruction" case a label placed at a program's end (with nothing
/// emitted after it) produces. See [`crate::Program::code_bit_len`]'s own
/// docs for why that position can be a valid target with no instruction
/// actually there.
///
/// # Errors
/// [`VmError::InvalidJumpTarget`] if `target` is neither `end_bit` nor any
/// instruction's `bit_offset` -- not something a program built through
/// [`crate::ProgramBuilder`] can produce (every target it ever writes is
/// one or the other), but `instrs`/`target` could in principle come from
/// anywhere.
pub fn resolve_jump_target(
    instrs: &[DecodedInstr],
    end_bit: u64,
    target: u64,
) -> Result<Option<usize>, VmError> {
    if target == end_bit {
        return Ok(None);
    }
    instrs
        .binary_search_by_key(&target, |i| i.bit_offset)
        .map(Some)
        .map_err(|_| VmError::InvalidJumpTarget(target))
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "-"),
            Self::ConstIndex(i) => write!(f, "const[{i}]"),
            Self::LocalSlot(s) => write!(f, "local[{s}]"),
            Self::JumpTarget(t) => write!(f, "@{t}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluff::interleave_fluff;
    use crate::program::ProgramBuilder;
    use crate::value::Value;

    fn loop_program() -> Program {
        // Same shape as gindema-vm's other loop tests: local[0] = 0..5,
        // summed via a backward jump -- real jump targets to exercise.
        let mut b = ProgramBuilder::new(1);
        let zero = b.add_const(Value::Int(0)).unwrap();
        let one = b.add_const(Value::Int(1)).unwrap();
        let five = b.add_const(Value::Int(5)).unwrap();
        b.load_const(zero).store_local(0);
        let top = b.new_label();
        let end = b.new_label();
        b.place_label(top);
        b.load_local(0).load_const(five).lt();
        b.jump_if_false(end);
        b.load_local(0).load_const(one).add().store_local(0);
        b.jump(top);
        b.place_label(end);
        b.load_local(0);
        b.halt();
        b.build().unwrap()
    }

    #[test]
    fn decodes_straight_line_arithmetic_in_order() {
        let mut b = ProgramBuilder::new(0);
        let two = b.add_const(Value::Int(2)).unwrap();
        let three = b.add_const(Value::Int(3)).unwrap();
        b.load_const(two).load_const(three).add().halt();
        let program = b.build().unwrap();

        let instrs = decode_program(&program).unwrap();
        let opcodes: Vec<OpCode> = instrs.iter().map(|i| i.opcode).collect();
        assert_eq!(
            opcodes,
            vec![
                OpCode::LoadConst,
                OpCode::LoadConst,
                OpCode::Add,
                OpCode::Halt
            ]
        );
        assert_eq!(instrs[0].operand, Operand::ConstIndex(two));
        assert_eq!(instrs[1].operand, Operand::ConstIndex(three));
        assert_eq!(instrs[2].operand, Operand::None);
        // Offsets strictly increase and the last one's end matches the
        // program's own declared length.
        let last = instrs.last().unwrap();
        assert_eq!(last.bit_offset + last.bit_len, program.code_bit_len());
    }

    #[test]
    fn decoded_jump_targets_point_at_another_instructions_bit_offset() {
        let program = loop_program();
        let instrs = decode_program(&program).unwrap();

        let jumps: Vec<&DecodedInstr> = instrs
            .iter()
            .filter(|i| matches!(i.opcode, OpCode::Jump | OpCode::JumpIfFalse))
            .collect();
        assert_eq!(jumps.len(), 2, "one backward Jump, one JumpIfFalse");

        for j in jumps {
            let Operand::JumpTarget(target) = j.operand else {
                panic!("Jump/JumpIfFalse always decodes to Operand::JumpTarget");
            };
            let resolved = resolve_jump_target(&instrs, program.code_bit_len(), target).unwrap();
            // Every jump in this particular program targets a real
            // instruction, never the implicit end.
            assert!(resolved.is_some());
            assert_eq!(instrs[resolved.unwrap()].bit_offset, target);
        }
    }

    #[test]
    fn resolve_jump_target_recognizes_the_implicit_end() {
        let mut b = ProgramBuilder::new(0);
        let end = b.new_label();
        b.jump(end);
        b.place_label(end);
        let program = b.build().unwrap();

        let instrs = decode_program(&program).unwrap();
        let Operand::JumpTarget(target) = instrs[0].operand else {
            panic!("the single instruction is the Jump itself");
        };
        assert_eq!(target, program.code_bit_len());
        assert_eq!(
            resolve_jump_target(&instrs, program.code_bit_len(), target).unwrap(),
            None
        );
    }

    #[test]
    fn resolve_jump_target_rejects_a_target_that_matches_no_instruction() {
        let program = ProgramBuilder::new(0).halt().build().unwrap();
        let instrs = decode_program(&program).unwrap();
        // 3 is mid-instruction (the single Halt occupies bits 0..6) and
        // isn't `end_bit` either.
        assert_eq!(
            resolve_jump_target(&instrs, program.code_bit_len(), 3),
            Err(VmError::InvalidJumpTarget(3))
        );
    }

    #[test]
    fn decode_program_transparently_skips_fluff() {
        let plain = loop_program();
        let fluffed = interleave_fluff(&plain, 0xF1FF).unwrap();
        assert!(fluffed.code_bit_len() > plain.code_bit_len());

        let plain_instrs = decode_program(&plain).unwrap();
        let fluffed_instrs = decode_program(&fluffed).unwrap();

        // Same opcodes/operands in the same order -- fluff changes bit
        // offsets (every instruction moves later in the stream) but not
        // the decoded shape at all.
        assert_eq!(plain_instrs.len(), fluffed_instrs.len());
        for (p, f) in plain_instrs.iter().zip(fluffed_instrs.iter()) {
            assert_eq!(p.opcode, f.opcode);
            // Jump targets differ in absolute value (the stream grew) but
            // every other operand kind (consts/locals) is unchanged.
            match (p.operand, f.operand) {
                (Operand::JumpTarget(_), Operand::JumpTarget(_)) => {}
                (a, b) => assert_eq!(a, b),
            }
        }
    }
}
