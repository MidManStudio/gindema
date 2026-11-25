// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "fluff.rs"
// ============================================================================

use gindema_vm::{interleave_fluff, Program};

use crate::opcode_remap::ObfuscateError;

/// State-dependent decoding's policy side: picks nothing more interesting
/// than passing `seed` straight through to [`interleave_fluff`], the actual
/// mechanism. Unlike per-build opcode remap — where this crate's real job
/// is generating a random *permutation* for `gindema_vm::OpcodeMap` to
/// validate — there's no comparable choice to make for fluff: a seed is a
/// seed, and every length 0..=7 it can produce before any instruction is
/// already as "random" as this pass gets. This wrapper exists so every
/// obfuscation pass is called the same way, through this crate, rather than
/// three of them living here and the fourth needing a direct
/// `gindema_vm` import.
///
/// # Errors
/// See [`interleave_fluff`]'s own errors, propagated via [`ObfuscateError::Vm`].
pub fn obfuscate_with_fluff(program: &Program, seed: u64) -> Result<Program, ObfuscateError> {
    Ok(interleave_fluff(program, seed)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gindema_vm::{ProgramBuilder, Value, Vm};

    #[test]
    fn wraps_interleave_fluff_and_still_runs_correctly() {
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
        let program = b.build().unwrap();

        let fluffed = obfuscate_with_fluff(&program, 0x1234).unwrap();
        assert_eq!(fluffed.fluff_seed(), Some(0x1234));
        assert!(fluffed.code_bit_len() >= program.code_bit_len());

        let mut vm = Vm::new();
        assert_eq!(vm.run(&fluffed).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn same_seed_is_deterministic() {
        let program = ProgramBuilder::new(0).halt().build().unwrap();
        let a = obfuscate_with_fluff(&program, 9).unwrap();
        let b = obfuscate_with_fluff(&program, 9).unwrap();
        assert_eq!(a.code_bytes(), b.code_bytes());
    }
}
