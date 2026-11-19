// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "lib.rs"
// ============================================================================

//! Compiler passes over `gindema-vm` bytecode. This first pass implements
//! per-build opcode remap: [`remap_opcodes`] re-encodes an already-built
//! [`gindema_vm::Program`] under a random, seed-derived opcode numbering, so
//! two builds of the same source program disagree about which raw byte
//! means which instruction, without changing the program's behavior, length,
//! or jump targets at all.
//!
//! The mechanism this leans on — [`gindema_vm::OpcodeMap`] and
//! [`gindema_vm::remap_program`] — lives in `gindema-vm` itself, since it
//! operates on that crate's own bytecode format; this crate owns the policy
//! (generating the actual permutation) and nothing else yet. Control-flow
//! flattening, state-dependent decoding, and honeypot/dead-end injection —
//! the rest of what this crate is meant to become — are not implemented in
//! this pass. See `docs/gindema-obfuscate.md`'s "Known limits".
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

mod opcode_remap;
mod rng;

pub use opcode_remap::{random_opcode_map, remap_opcodes, ObfuscateError};
