// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "honeypot.rs"
// ============================================================================

use gindema_vm::{BuildError, ProgramBuilder, Value};

use gindema_common::SplitMix64;

/// Emits a honeypot/dead-end block into `builder` at the current position:
/// a conditional branch that looks like real logic but is always taken the
/// same way, guarding a block of plausible-looking fake computation that
/// ends in its own `Halt` — a dead end that is never actually reached.
///
/// Built entirely on `gindema_vm::ProgramBuilder`'s existing label/jump API
/// — no changes to `gindema-vm` itself were needed for this, unlike the
/// opcode remap pass. Call this at any point while constructing a program
/// (interleaved with real instructions) to sprinkle one honeypot in; call
/// it more than once for more than one. `seed` controls which decoy
/// constants and which fake arithmetic the dead end contains — different
/// seeds produce different-looking (but equally dead) blocks, mainly so
/// several honeypots in the same program don't all look identical.
///
/// # How the branch is always-taken without being `push true; push true`
/// Two *different* constant-pool entries are added, both holding the same
/// value (`decoy_value`), so the branch reads as a genuine comparison
/// between two named values rather than an obviously-constant-folded
/// no-op. `Eq` on them is always `true`; `Not` makes it always `false`;
/// `JumpIfFalse` always jumps on `false` — so control always takes the
/// skip, every time, regardless of any runtime state. See
/// `docs/gindema-obfuscate.md` for how strong (or not) this actually is.
///
/// # Errors
/// [`BuildError::TooManyConstants`] if the program's constant pool is
/// already essentially full (this adds a handful more entries).
pub fn inject_honeypot(builder: &mut ProgramBuilder, seed: u64) -> Result<(), BuildError> {
    let mut rng = SplitMix64::new(seed);

    let decoy_value = rng.next_below(1_000_000) as i64;
    let a = builder.add_const(Value::Int(decoy_value))?;
    let b = builder.add_const(Value::Int(decoy_value))?;

    let skip = builder.new_label();
    builder
        .load_const(a)
        .load_const(b)
        .eq()
        .not()
        .jump_if_false(skip);

    // Dead end: a few steps of plausible-looking fake arithmetic, ending in
    // its own `Halt` as if it were a legitimate early exit. Never actually
    // reached — the branch above always takes the skip above instead.
    let step_count = 2 + rng.next_below(3); // 2..=4 steps
    for _ in 0..step_count {
        let lhs = rng.next_below(1000) as i64;
        let rhs = rng.next_below(1000) as i64;
        let ci = builder.add_const(Value::Int(lhs))?;
        let cj = builder.add_const(Value::Int(rhs))?;
        builder.load_const(ci).load_const(cj);
        match rng.next_below(3) {
            0 => builder.add(),
            1 => builder.sub(),
            _ => builder.mul(),
        };
    }
    builder.halt();

    builder.place_label(skip);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gindema_vm::Vm;

    #[test]
    fn honeypot_is_never_actually_reached() {
        let mut b = ProgramBuilder::new(0);
        let one = b.add_const(Value::Int(1)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();

        b.load_const(one);
        inject_honeypot(&mut b, 42).unwrap();
        b.load_const(two).add().halt();
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        // If the branch-always-skips logic above were ever wrong and the
        // dead end actually ran, the program would `Halt` inside it with
        // whatever fake arithmetic it left on the stack, well before this
        // real `load_const(two).add()` ever runs. Getting exactly 3 back
        // proves the honeypot was skipped, not just that the stack
        // happened to look right afterward.
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(3)));
    }

    #[test]
    fn honeypot_actually_adds_instructions_not_a_no_op() {
        let mut without = ProgramBuilder::new(0);
        without.halt();
        let without = without.build().unwrap();

        let mut with = ProgramBuilder::new(0);
        inject_honeypot(&mut with, 1).unwrap();
        with.halt();
        let with = with.build().unwrap();

        assert!(with.code_len_bytes() > without.code_len_bytes());
    }

    #[test]
    fn same_seed_produces_the_same_bytes_different_seeds_usually_differ() {
        let mut a = ProgramBuilder::new(0);
        inject_honeypot(&mut a, 7).unwrap();
        a.halt();
        let a = a.build().unwrap();

        let mut b = ProgramBuilder::new(0);
        inject_honeypot(&mut b, 7).unwrap();
        b.halt();
        let b = b.build().unwrap();
        assert_eq!(a.code_bytes(), b.code_bytes());

        let mut c = ProgramBuilder::new(0);
        inject_honeypot(&mut c, 8).unwrap();
        c.halt();
        let c = c.build().unwrap();
        assert_ne!(a.code_bytes(), c.code_bytes());
    }

    #[test]
    fn multiple_honeypots_in_one_program_all_stay_unreachable() {
        let mut b = ProgramBuilder::new(0);
        let five = b.add_const(Value::Int(5)).unwrap();
        inject_honeypot(&mut b, 1).unwrap();
        b.load_const(five);
        inject_honeypot(&mut b, 2).unwrap();
        inject_honeypot(&mut b, 3).unwrap();
        b.halt();
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(5)));
    }
}
