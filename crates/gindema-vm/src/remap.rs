// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "remap.rs"
// ============================================================================

use gindema_bits::{BitReader, BitWriter};

use crate::error::VmError;
use crate::opcode::{OpCode, OPCODE_WIDTH};
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
/// `gindema-obfuscate`'s per-build opcode remap is the intended caller:
/// generate a random permutation, build an [`OpcodeMap`] from it, and
/// remap the program compiled from the one fixed `gindema-vm` baseline.
/// This function itself has no opinion on how `new_map` was chosen.
///
/// # Errors
/// [`VmError::InvalidOpcode`] if a raw opcode byte in `program`'s stream
/// doesn't decode under `program.opcode_map()` — a malformed program, not
/// something a program built through [`crate::ProgramBuilder`] can
/// produce. Propagates [`VmError::Bits`] if the stream is truncated.
pub fn remap_program(program: &Program, new_map: &OpcodeMap) -> Result<Program, VmError> {
    let mut reader = BitReader::new(program.code_bytes());
    let mut writer = BitWriter::new();

    loop {
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

        if opcode == OpCode::Halt {
            break;
        }
    }

    Ok(Program {
        code: writer.into_bytes(),
        constants: program.constants().to_vec(),
        local_count: program.local_count(),
        opcode_map: new_map.clone(),
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
