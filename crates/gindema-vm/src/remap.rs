// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "remap.rs"
// ============================================================================

use gindema_bits::{BitReader, BitWriter};

use crate::error::VmError;
use crate::fluff::copy_fluff;
use crate::opcode::OPCODE_WIDTH;
use crate::opcode_map::OpcodeMap;
use crate::program::Program;

/// Re-encodes `program`'s instruction stream under a different
/// [`OpcodeMap`] — the same logic, constants, and local count, but a
/// different raw byte meaning each opcode.
///
/// This is a pure, lossless, instruction-by-instruction transcoding, not a
/// decode-and-rebuild through [`crate::ProgramBuilder`]: an opcode's
/// operand *width* is fixed by the opcode itself and does not depend on
/// which [`OpcodeMap`] is in use (only which raw value the 6-bit opcode
/// field holds does), so every instruction occupies exactly the same
/// number of bits, at exactly the same stream position, before and after
/// remapping. That in turn means every `Jump`/`JumpIfFalse` target — an
/// absolute bit offset into the stream — stays valid completely unchanged;
/// there's nothing to recompute. Each instruction's opcode field is
/// decoded under `program`'s current map, re-encoded under `new_map`, and
/// its operand bits (if any) are copied through verbatim.
///
/// Walks every instruction up to exactly [`Program::code_bit_len`] bits —
/// **not** until the first `Halt` opcode. An earlier version of this
/// function stopped at the first `Halt` it decoded, which is correct for
/// [`crate::Vm::run`] (which only ever reaches a `Halt` by actually
/// executing its way there) but silently truncated any program containing
/// dead code with its own `Halt` positioned earlier in the raw byte stream
/// than the program's real one — exactly what a `gindema-obfuscate`
/// honeypot's dead end is. This function never follows control flow, so it
/// has no way to tell a reachable `Halt` from a dead one; decoding by exact
/// bit length rather than by opcode sidesteps needing to.
///
/// `gindema-obfuscate`'s per-build opcode remap is the intended caller:
/// generate a random permutation, build an [`OpcodeMap`] from it, and
/// remap the program compiled from the one fixed `gindema-vm` baseline.
/// This function itself has no opinion on how `new_map` was chosen.
///
/// Fluff-aware (see `docs/gindema-vm.md`'s `fluff.rs` section): if
/// `program` carries fluff, each region is read and copied through to the
/// output completely unchanged (via [`crate::fluff::copy_fluff`]) rather
/// than being misread as a bogus opcode — opcode remapping and fluff are
/// independent layers, so remapping never needs to regenerate or move
/// fluff, only step over it correctly. The output keeps the same
/// [`Program::fluff_seed`] as the input.
///
/// # Errors
/// [`VmError::InvalidOpcode`] if a raw opcode byte in `program`'s stream
/// doesn't decode under `program.opcode_map()` — a malformed program, not
/// something a program built through [`crate::ProgramBuilder`] can
/// produce. Propagates [`VmError::Bits`] if the stream is truncated.
pub fn remap_program(program: &Program, new_map: &OpcodeMap) -> Result<Program, VmError> {
    let mut reader = BitReader::new(program.code_bytes());
    let mut writer = BitWriter::new();
    let end_bit = program.code_bit_len();

    while reader.bit_pos() < end_bit {
        copy_fluff(&mut reader, &mut writer, program)?;
        let raw = reader.read_bits(OPCODE_WIDTH)? as u8;
        let opcode = program
            .opcode_map()
            .decode(raw)
            .ok_or(VmError::InvalidOpcode(raw))?;

        writer
            .write_bits(u64::from(new_map.encode(opcode)), OPCODE_WIDTH)
            .expect("OpcodeMap only ever encodes to a value that fits in OPCODE_WIDTH bits");

        let width = opcode.operand_width();
        if width > 0 {
            let operand = reader.read_bits(width)?;
            writer
                .write_bits(operand, width)
                .expect("operand bits are copied at their original width, which already fit");
        }
    }

    Ok(Program {
        code: writer.into_bytes(),
        code_bit_len: end_bit,
        constants: program.constants().to_vec(),
        local_count: program.local_count(),
        opcode_map: new_map.clone(),
        fluff_seed: program.fluff_seed(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::ProgramBuilder;
    use crate::value::Value;
    use crate::vm::Vm;

    fn scrambled_map() -> OpcodeMap {
        let mut raw_values = [0u8; crate::opcode::OPCODE_COUNT];
        for (i, v) in raw_values.iter_mut().enumerate() {
            // Spread the 17 opcodes out across the 64-slot space instead of
            // a tight 0..=16 block, and in reverse order, so the test can't
            // pass by accident if remapping silently no-ops.
            *v = (63 - i * 3) as u8;
        }
        OpcodeMap::from_permutation(raw_values).unwrap()
    }

    #[test]
    fn remapped_program_encodes_differently_but_runs_identically() {
        let mut b = ProgramBuilder::new(1);
        let five = b.add_const(Value::Int(5)).unwrap();
        let one = b.add_const(Value::Int(1)).unwrap();
        let zero = b.add_const(Value::Int(0)).unwrap();

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
        let original = b.build().unwrap();

        let new_map = scrambled_map();
        let remapped = remap_program(&original, &new_map).unwrap();

        // Same length (every instruction keeps its exact width), different
        // bytes (the opcode field's raw values actually changed), same
        // observable behavior.
        assert_eq!(remapped.code_bytes().len(), original.code_bytes().len());
        assert_ne!(remapped.code_bytes(), original.code_bytes());
        assert_eq!(remapped.opcode_map(), &new_map);

        let mut vm_original = Vm::new();
        let mut vm_remapped = Vm::new();
        assert_eq!(
            vm_original.run(&original).unwrap(),
            vm_remapped.run(&remapped).unwrap()
        );
    }

    #[test]
    fn remap_does_not_truncate_at_an_unreached_halt_in_dead_code() {
        // Reproduces, inside this crate (gindema-vm can't depend on
        // gindema-obfuscate to use its real inject_honeypot), the exact
        // shape that first caught this bug: a branch that's always taken,
        // skipping over dead code which itself contains a `Halt` -- placed
        // in the byte stream *before* the real, reachable code that
        // follows. The old Halt-stops-decoding version of remap_program
        // would truncate here and never reach this test's final
        // `load_const(two).halt()` at all.
        let mut b = ProgramBuilder::new(0);
        let one = b.add_const(Value::Int(1)).unwrap();
        let fake = b.add_const(Value::Int(999)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();

        // An always-false condition (built from a literal, not from any
        // runtime state) so `jump_if_false` always takes the skip.
        let always_false = b.add_const(Value::Bool(false)).unwrap();
        let skip = b.new_label();
        b.load_const(always_false).jump_if_false(skip);
        // Dead code: never reached, but it has its own Halt, positioned
        // earlier in the stream than the real one below.
        b.load_const(fake).halt();
        b.place_label(skip);
        // Real, reachable code, physically after the dead Halt above.
        b.load_const(one).load_const(two).add().halt();
        let original = b.build().unwrap();

        let remapped = remap_program(&original, &scrambled_map()).unwrap();
        assert_eq!(remapped.code_bit_len(), original.code_bit_len());

        let mut vm = Vm::new();
        assert_eq!(vm.run(&remapped).unwrap(), Some(Value::Int(3)));
    }

    #[test]
    fn remapping_twice_with_the_same_map_is_a_no_op_on_the_bytes() {
        let program = ProgramBuilder::new(0).halt().build().unwrap();
        let map = OpcodeMap::identity();
        let remapped = remap_program(&program, &map).unwrap();
        assert_eq!(remapped.code_bytes(), program.code_bytes());
    }

    #[test]
    fn remapping_back_to_identity_recovers_the_original_bytes() {
        let mut b = ProgramBuilder::new(0);
        let two = b.add_const(Value::Int(2)).unwrap();
        b.load_const(two).neg().halt();
        let original = b.build().unwrap();

        let scrambled = remap_program(&original, &scrambled_map()).unwrap();
        assert_ne!(scrambled.code_bytes(), original.code_bytes());

        let back_to_identity = remap_program(&scrambled, &OpcodeMap::identity()).unwrap();
        assert_eq!(back_to_identity.code_bytes(), original.code_bytes());
    }
}
