// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "lib.rs"
// ============================================================================

//! Compiler passes over `gindema-vm` bytecode. Two of the four passes this
//! crate is meant to become are implemented:
//!
//! - **Per-build opcode remap** — [`remap_opcodes`] re-encodes an
//!   already-built [`gindema_vm::Program`] under a random, seed-derived
//!   opcode numbering, so two builds of the same source program disagree
//!   about which raw byte means which instruction, without changing the
//!   program's behavior, length, or jump targets at all. The mechanism
//!   this leans on — [`gindema_vm::OpcodeMap`] and
//!   [`gindema_vm::remap_program`] — lives in `gindema-vm` itself, since it
//!   operates on that crate's own bytecode format; this crate owns the
//!   policy (generating the actual permutation).
//! - **Honeypot/dead-end injection** — [`inject_honeypot`] emits a
//!   conditional branch that looks like real logic but always takes the
//!   same path, guarding a block of plausible fake computation that ends
//!   in its own `Halt` and is never actually reached. Built entirely on
//!   `gindema_vm::ProgramBuilder`'s existing label/jump API — no changes to
//!   `gindema-vm` were needed for this one.
//!
//! - **Control-flow flattening** — [`flatten_control_flow`] decodes an
//!   already-built program (via [`gindema_vm::decode_program`]) into basic
//!   blocks and rebuilds it as a single dispatcher loop, so every inter-block
//!   transfer — including an ordinary block's own fallthrough into the next
//!   one — reads as "set a state local, jump back to the dispatcher"
//!   instead of a direct jump wherever the destination happens to sit.
//! - **State-dependent decoding** — [`obfuscate_with_fluff`] is a thin
//!   wrapper over [`gindema_vm::interleave_fluff`], the actual mechanism,
//!   which needed a real `gindema-vm` format change (unlike every other
//!   pass here): interleaving junk bits before each instruction moves every
//!   later jump target, so something has to recompute them.
//!
//! See `docs/gindema-obfuscate.md`'s "Known limits" for how strong (or not)
//! all four passes actually are.
//!
//! # Example
//!
//! ```
//! use gindema_obfuscate::remap_opcodes;
//! use gindema_vm::{ProgramBuilder, Value, Vm};
//!
//! let mut builder = ProgramBuilder::new(0);
//! let two = builder.add_const(Value::Int(2)).unwrap();
//! let three = builder.add_const(Value::Int(3)).unwrap();
//! builder.load_const(two).load_const(three).add().halt();
//! let program = builder.build().unwrap();
//!
//! let remapped = remap_opcodes(&program, 0xC0FFEE).unwrap();
//! assert_ne!(remapped.code_bytes(), program.code_bytes());
//!
//! let mut vm = Vm::new();
//! assert_eq!(vm.run(&remapped).unwrap(), Some(Value::Int(5)));
//! ```
//!
//! ```
//! use gindema_obfuscate::inject_honeypot;
//! use gindema_vm::{ProgramBuilder, Value, Vm};
//!
//! let mut builder = ProgramBuilder::new(0);
//! let one = builder.add_const(Value::Int(1)).unwrap();
//! builder.load_const(one);
//! inject_honeypot(&mut builder, 0xBEE5).unwrap(); // never actually runs
//! builder.halt();
//! let program = builder.build().unwrap();
//!
//! let mut vm = Vm::new();
//! assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(1)));
//! ```
//!
//! ```
//! use gindema_obfuscate::flatten_control_flow;
//! use gindema_vm::{ProgramBuilder, Value, Vm};
//!
//! // if (2 == 2) { 10 } else { 20 } -- one real branch to flatten.
//! let mut builder = ProgramBuilder::new(0);
//! let two_a = builder.add_const(Value::Int(2)).unwrap();
//! let two_b = builder.add_const(Value::Int(2)).unwrap();
//! let ten = builder.add_const(Value::Int(10)).unwrap();
//! let twenty = builder.add_const(Value::Int(20)).unwrap();
//! let else_branch = builder.new_label();
//! let end = builder.new_label();
//! builder.load_const(two_a).load_const(two_b).eq();
//! builder.jump_if_false(else_branch);
//! builder.load_const(ten);
//! builder.jump(end);
//! builder.place_label(else_branch);
//! builder.load_const(twenty);
//! builder.place_label(end);
//! builder.halt();
//! let program = builder.build().unwrap();
//!
//! let flattened = flatten_control_flow(&program, 0xF1A7).unwrap();
//! assert_ne!(flattened.code_bytes(), program.code_bytes());
//!
//! let mut vm = Vm::new();
//! assert_eq!(vm.run(&flattened).unwrap(), Some(Value::Int(10)));
//! ```
//!
//! ```
//! use gindema_obfuscate::obfuscate_with_fluff;
//! use gindema_vm::{ProgramBuilder, Value, Vm};
//!
//! let mut builder = ProgramBuilder::new(0);
//! let two = builder.add_const(Value::Int(2)).unwrap();
//! builder.load_const(two).halt();
//! let program = builder.build().unwrap();
//!
//! let fluffed = obfuscate_with_fluff(&program, 0x51EED).unwrap();
//! assert!(fluffed.code_bit_len() >= program.code_bit_len());
//!
//! let mut vm = Vm::new();
//! assert_eq!(vm.run(&fluffed).unwrap(), Some(Value::Int(2)));
//! ```

mod flatten;
mod fluff;
mod honeypot;
mod opcode_remap;

pub use flatten::flatten_control_flow;
pub use fluff::obfuscate_with_fluff;
pub use honeypot::inject_honeypot;
pub use opcode_remap::{random_opcode_map, remap_opcodes, ObfuscateError};

#[cfg(test)]
mod tests {
    //! Composability tests: each pass above has its own thorough tests
    //! against the public surface it adds, but nothing in any single
    //! module's tests exercises all four passes chained together the way a
    //! real build pipeline eventually will. This module is specifically
    //! for that — only ever calling this crate's own public functions, the
    //! same as any external caller would.

    use gindema_vm::{Program, ProgramBuilder, Value, Vm};

    use super::*;

    fn program_with_a_loop_and_a_honeypot() -> Program {
        let mut b = ProgramBuilder::new(1);
        let zero = b.add_const(Value::Int(0)).unwrap();
        let one = b.add_const(Value::Int(1)).unwrap();
        let five = b.add_const(Value::Int(5)).unwrap();
        b.load_const(zero).store_local(0);
        inject_honeypot(&mut b, 0xA11CE).unwrap();
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
    fn honeypot_then_flatten_then_fluff_then_remap_all_compose() {
        let program = program_with_a_loop_and_a_honeypot();

        let flattened = flatten_control_flow(&program, 1).unwrap();
        let fluffed = obfuscate_with_fluff(&flattened, 2).unwrap();
        let remapped = remap_opcodes(&fluffed, 3).unwrap();

        assert_eq!(remapped.fluff_seed(), Some(2));
        assert_eq!(remapped.opcode_map(), &random_opcode_map(3));

        let mut baseline = Vm::new();
        let mut piped = Vm::new();
        assert_eq!(
            baseline.run(&program).unwrap(),
            piped.run(&remapped).unwrap()
        );
        assert_eq!(piped.run(&remapped).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn the_reverse_order_within_the_fixed_flatten_first_constraint_also_composes() {
        // fluff -> flatten still has to flatten first internally (flatten
        // always decodes away any existing fluff/remap and rebuilds clean
        // -- see flatten_control_flow's own docs) but the *caller* calling
        // obfuscate_with_fluff on an ALREADY-flattened program, rather than
        // flattening last, should still produce a correct program; this is
        // the ordering docs/gindema-obfuscate.md recommends against for
        // the fluff-then-flatten direction specifically, confirmed here to
        // still at least run correctly (just discarding the fluff) rather
        // than silently producing something wrong.
        let program = program_with_a_loop_and_a_honeypot();
        let fluffed_first = obfuscate_with_fluff(&program, 9).unwrap();
        let flattened_after = flatten_control_flow(&fluffed_first, 10).unwrap();

        assert_eq!(flattened_after.fluff_seed(), None);
        let mut vm = Vm::new();
        assert_eq!(vm.run(&flattened_after).unwrap(), Some(Value::Int(5)));
    }
}
