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
//! Control-flow flattening and state-dependent decoding are not
//! implemented in this pass. See `docs/gindema-obfuscate.md`'s "Known
//! limits" for both of those and for how strong (or not) the two
//! implemented passes actually are.
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

mod honeypot;
mod opcode_remap;
mod rng;

pub use honeypot::inject_honeypot;
pub use opcode_remap::{random_opcode_map, remap_opcodes, ObfuscateError};
