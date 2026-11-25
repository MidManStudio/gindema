// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "fluff.rs"
// ============================================================================

//! State-dependent decoding's bytecode-format mechanism: interleaving
//! pseudo-random junk ("fluff") bits between instructions, where the amount
//! of fluff before any given instruction is never stored explicitly in the
//! stream -- it's derived, identically by the encoder and by
//! [`crate::Vm::run`], from the seed and the instruction's own bit
//! position. See [`interleave_fluff`]'s docs for the full design and why
//! bit *position* rather than instruction *index* is the key that makes
//! this work across jumps.
//!
//! `gindema-obfuscate` owns the *policy* here too (picking a seed), same
//! split as opcode remap — but unlike opcode remap, there's no interesting
//! permutation space to choose from, so that wrapper is nearly a direct
//! pass-through. Everything that actually needed a bytecode-format change
//! lives here.

use gindema_bits::{BitReader, BitWriter};

use gindema_common::SplitMix64;

use crate::decode::{decode_program, resolve_jump_target, Operand};
use crate::error::VmError;
use crate::opcode::{CONST_INDEX_WIDTH, JUMP_TARGET_WIDTH, LOCAL_SLOT_WIDTH, OPCODE_WIDTH};
use crate::program::Program;

/// The maximum number of fluff bits that can precede any one instruction.
/// Deliberately small — average overhead is `MAX_FLUFF_BITS / 2` bits per
/// instruction, so a long program doesn't balloon in size for a pass whose
/// whole point is to be a cheap, composable layer under opcode remap and
/// flattening, not a payload in its own right.
pub(crate) const MAX_FLUFF_BITS: u32 = 7;

/// How many fluff bits precede the instruction whose decoding is about to
/// start at `bit_pos`, under `seed` — a pure function of `(seed, bit_pos)`,
/// nothing else. This one property is what makes jumping into fluffed
/// bytecode work at all: wherever [`gindema_bits::BitReader`]'s cursor
/// lands — by falling through sequentially, or by
/// [`gindema_bits::BitReader::seek_to_bit`] from a `Jump`/`JumpIfFalse` —
/// this always recovers the exact fluff length the encoder used there,
/// with no per-instruction index, counter, or lookup table that would
/// otherwise have to be threaded through every jump target, growing the
/// format a second way on top of the fluff itself.
pub(crate) fn fluff_len_at(seed: u64, bit_pos: u64) -> u32 {
    let mut rng = SplitMix64::new(seed ^ bit_pos);
    rng.next_below(MAX_FLUFF_BITS as usize + 1) as u32
}

/// The actual junk bits written for a `len`-bit fluff region at `bit_pos`.
/// Only *length* needs to match between encode and decode for correctness
/// (see [`fluff_len_at`]) — content is never read back by anything, so this
/// only has to be *some* deterministic `len`-bit value, for round-trip
/// testing's sake and so the junk isn't suspiciously all-zero. A distinct
/// domain-separation constant keeps it from being a trivial function of the
/// length alone.
fn fluff_bits_at(seed: u64, bit_pos: u64, len: u32) -> u64 {
    debug_assert!(len <= MAX_FLUFF_BITS);
    let mut rng = SplitMix64::new(seed ^ bit_pos ^ 0x464C_5546_4621);
    rng.next_below(1usize << len) as u64
}

/// Skips this position's fluff on `reader`, if `program` carries any — the
/// one piece of logic [`crate::Vm::run`], [`crate::decode_program`], and
/// [`crate::remap_program`] all need identically before decoding whatever
/// instruction sits at the reader's current position. Pulled out here
/// rather than duplicated three times, since all three living in sync is
/// exactly what fluff's correctness depends on.
pub(crate) fn skip_fluff(reader: &mut BitReader<'_>, program: &Program) -> Result<(), VmError> {
    if let Some(seed) = program.fluff_seed() {
        let flen = fluff_len_at(seed, reader.bit_pos());
        if flen > 0 {
            reader.read_bits(flen)?;
        }
    }
    Ok(())
}

/// Reads this position's fluff on `reader` (if `program` carries any) and
/// writes the exact same bits straight to `writer` — [`crate::remap_program`]'s
/// use case: a pass that re-encodes a program's real instructions but must
/// leave any existing fluff physically unchanged, since fluff's length is a
/// function of bit *position*, and remapping opcodes never moves anything.
pub(crate) fn copy_fluff(
    reader: &mut BitReader<'_>,
    writer: &mut BitWriter,
    program: &Program,
) -> Result<(), VmError> {
    if let Some(seed) = program.fluff_seed() {
        let flen = fluff_len_at(seed, reader.bit_pos());
        if flen > 0 {
            let junk = reader.read_bits(flen)?;
            writer
                .write_bits(junk, flen)
                .expect("junk bits read at width `flen` always fit back into the same width");
        }
    }
    Ok(())
}

/// Interleaves pseudo-random fluff bits before every instruction in
/// `program`'s stream, derived from `seed` as [`fluff_len_at`] describes,
/// and rewrites every `Jump`/`JumpIfFalse` target to the same instruction's
/// new (fluff-inflated) bit offset — the actual reason this needed a
/// `gindema-vm` format change rather than living entirely in
/// `gindema-obfuscate` the way honeypot injection does: moving instructions
/// later in the stream invalidates every absolute bit-offset jump target
/// after the first fluff region, so something has to recompute them, and
/// that something needs the same byte-and-bit-level knowledge
/// [`crate::remap_program`] already needed for a much simpler reason.
///
/// Built on [`decode_program`] (so it transparently re-fluffs an
/// already-fluffed program under a new seed, discarding the old fluff,
/// rather than stacking two independent fluff layers) and
/// [`resolve_jump_target`] (so a malformed jump target is rejected the same
/// way in every pass that uses it, not reimplemented per pass).
///
/// Two-pass, the same shape as [`crate::ProgramBuilder::build`]: a first
/// pass walks the decoded instructions computing each one's new bit offset
/// (which also pins down the total new length, and therefore every fluff
/// length along the way, before a single real bit is written); a second
/// pass actually writes fluff, opcode, and operand for each instruction,
/// with every jump target already resolved to its destination's new offset.
///
/// # Errors
/// Propagates [`VmError`] from [`decode_program`] if `program`'s stream is
/// malformed. [`VmError::ProgramTooLarge`] if the fluff-inflated stream
/// would exceed a 32-bit jump-target range.
pub fn interleave_fluff(program: &Program, seed: u64) -> Result<Program, VmError> {
    let instrs = decode_program(program)?;
    let end_bit = program.code_bit_len();

    // Pass 1: fluff interleaving never reorders instructions, only inserts
    // junk between them, so instruction `i`'s relative order is unchanged
    // -- only its absolute bit offset grows by however much fluff precedes
    // it (including fluff before every earlier instruction too).
    let mut new_offsets: Vec<u64> = Vec::with_capacity(instrs.len());
    let mut cursor: u64 = 0;
    for instr in &instrs {
        new_offsets.push(cursor);
        let flen = u64::from(fluff_len_at(seed, cursor));
        cursor += flen + u64::from(OPCODE_WIDTH) + u64::from(instr.opcode.operand_width());
    }
    let new_end_bit = cursor;
    if new_end_bit > u64::from(u32::MAX) {
        return Err(VmError::ProgramTooLarge);
    }

    // Resolves an ORIGINAL jump target (a bit offset in `program`'s own
    // stream) to its NEW offset in the fluff-inflated stream being built.
    let resolve_new = |target: u64| -> Result<u64, VmError> {
        match resolve_jump_target(&instrs, end_bit, target)? {
            Some(idx) => Ok(new_offsets[idx]),
            None => Ok(new_end_bit),
        }
    };

    // Pass 2: write fluff, then the real instruction, for each one in turn.
    let mut writer = BitWriter::new();
    for (i, instr) in instrs.iter().enumerate() {
        let fluff_start = new_offsets[i];
        debug_assert_eq!(writer.bit_len(), fluff_start);
        let flen = fluff_len_at(seed, fluff_start);
        if flen > 0 {
            let junk = fluff_bits_at(seed, fluff_start, flen);
            writer
                .write_bits(junk, flen)
                .expect("fluff_bits_at never returns a value wider than `len` bits");
        }

        writer
            .write_bits(
                u64::from(program.opcode_map().encode(instr.opcode)),
                OPCODE_WIDTH,
            )
            .expect("an opcode byte always fits in OPCODE_WIDTH bits");

        match instr.operand {
            Operand::None => {}
            Operand::ConstIndex(idx) => writer
                .write_bits(u64::from(idx), CONST_INDEX_WIDTH)
                .expect("a constant-pool index is always within CONST_INDEX_WIDTH bits"),
            Operand::LocalSlot(slot) => writer
                .write_bits(u64::from(slot), LOCAL_SLOT_WIDTH)
                .expect("a local-slot index is always within LOCAL_SLOT_WIDTH bits"),
            Operand::JumpTarget(target) => {
                let new_target = resolve_new(target)?;
                writer
                    .write_bits(new_target, JUMP_TARGET_WIDTH)
                    .expect("new_end_bit was already checked against u32::MAX above");
            }
        }
    }

    Ok(Program {
        code: writer.into_bytes(),
        code_bit_len: new_end_bit,
        constants: program.constants().to_vec(),
        local_count: program.local_count(),
        opcode_map: program.opcode_map().clone(),
        fluff_seed: Some(seed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opcode_map::OpcodeMap;
    use crate::program::ProgramBuilder;
    use crate::remap::remap_program;
    use crate::value::Value;
    use crate::vm::Vm;

    fn loop_program() -> Program {
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
    fn fluffed_program_grows_and_carries_its_seed() {
        let original = loop_program();
        let fluffed = interleave_fluff(&original, 0xC0FFEE).unwrap();

        assert!(fluffed.code_bit_len() >= original.code_bit_len());
        assert_eq!(fluffed.fluff_seed(), Some(0xC0FFEE));
        assert_eq!(original.fluff_seed(), None);
    }

    #[test]
    fn fluffed_program_runs_identically_including_its_loop() {
        let original = loop_program();
        let fluffed = interleave_fluff(&original, 42).unwrap();

        let mut vm_a = Vm::new();
        let mut vm_b = Vm::new();
        assert_eq!(vm_a.run(&original).unwrap(), vm_b.run(&fluffed).unwrap());
        assert_eq!(vm_b.run(&fluffed).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn same_seed_is_deterministic_different_seeds_usually_differ() {
        let original = loop_program();
        let a = interleave_fluff(&original, 7).unwrap();
        let b = interleave_fluff(&original, 7).unwrap();
        assert_eq!(a.code_bytes(), b.code_bytes());

        let c = interleave_fluff(&original, 8).unwrap();
        assert_ne!(a.code_bytes(), c.code_bytes());
    }

    #[test]
    fn re_fluffing_an_already_fluffed_program_replaces_the_old_fluff() {
        let original = loop_program();
        let once = interleave_fluff(&original, 1).unwrap();
        let twice = interleave_fluff(&once, 2).unwrap();

        assert_eq!(twice.fluff_seed(), Some(2));
        let mut vm = Vm::new();
        assert_eq!(vm.run(&twice).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn fluffing_a_program_containing_a_dead_branch_still_decodes_the_real_tail() {
        // Mirrors remap.rs's own honeypot-shaped regression test: a
        // dead-end with its own Halt, positioned before the real code.
        let mut b = ProgramBuilder::new(0);
        let one = b.add_const(Value::Int(1)).unwrap();
        let fake = b.add_const(Value::Int(999)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();
        let always_false = b.add_const(Value::Bool(false)).unwrap();
        let skip = b.new_label();
        b.load_const(always_false).jump_if_false(skip);
        b.load_const(fake).halt();
        b.place_label(skip);
        b.load_const(one).load_const(two).add().halt();
        let original = b.build().unwrap();

        let fluffed = interleave_fluff(&original, 99).unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&fluffed).unwrap(), Some(Value::Int(3)));
    }

    #[test]
    fn remapping_a_fluffed_program_preserves_its_fluff_and_behavior() {
        let mut raw_values = [0u8; crate::opcode::OPCODE_COUNT];
        for (i, v) in raw_values.iter_mut().enumerate() {
            *v = (63 - i * 3) as u8;
        }
        let map = OpcodeMap::from_permutation(raw_values).unwrap();

        let original = loop_program();
        let fluffed = interleave_fluff(&original, 55).unwrap();
        let remapped = remap_program(&fluffed, &map).unwrap();

        assert_eq!(remapped.fluff_seed(), Some(55));
        assert_eq!(remapped.opcode_map(), &map);
        let mut vm = Vm::new();
        assert_eq!(vm.run(&remapped).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn a_fluffed_program_round_trips_through_to_bytes() {
        let fluffed = interleave_fluff(&loop_program(), 3).unwrap();
        let restored = Program::from_bytes(&fluffed.to_bytes()).unwrap();
        assert_eq!(restored, fluffed);
        let mut vm = Vm::new();
        assert_eq!(vm.run(&restored).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn fluff_len_at_never_exceeds_the_max() {
        for bit_pos in 0..500u64 {
            assert!(fluff_len_at(0xABCD, bit_pos) <= MAX_FLUFF_BITS);
        }
    }

    #[test]
    fn fluff_bits_at_never_exceeds_its_requested_width() {
        for len in 0..=MAX_FLUFF_BITS {
            let value = fluff_bits_at(123, 456, len);
            assert!(value < (1u64 << len).max(1));
        }
    }
}
