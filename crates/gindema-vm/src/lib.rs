// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "lib.rs"
// ============================================================================

//! Custom stack-based bytecode format and interpreter. The core virtual
//! machine the rest of the gindema workspace builds on: `gindema-obfuscate`
//! will compile into it, `gindema-guard` will watch over it running.
//!
//! Opcodes are invented for this project specifically (see [`OpCode`]) so
//! there is no shared alphabet with any real ISA for a decompiler to
//! recognize. Per-build opcode remapping, control-flow flattening, and
//! state-dependent decoding are `gindema-obfuscate`'s job, layered on top of
//! this fixed baseline instruction set — not this crate's.
//!
//! This first pass is deliberately scoped to a correct, safe, well-tested
//! core rather than a complete one: see `docs/gindema-vm.md`'s "Known
//! limits" for what's intentionally not here yet (functions/calls, more
//! value types, an assembly syntax, any obfuscation).
//!
//! # Example
//!
//! ```
//! use gindema_vm::{ProgramBuilder, Value, Vm};
//!
//! // (2 + 3) * 4
//! let mut builder = ProgramBuilder::new(0);
//! let two = builder.add_const(Value::Int(2)).unwrap();
//! let three = builder.add_const(Value::Int(3)).unwrap();
//! let four = builder.add_const(Value::Int(4)).unwrap();
//! builder
//!     .load_const(two)
//!     .load_const(three)
//!     .add()
//!     .load_const(four)
//!     .mul()
//!     .halt();
//! let program = builder.build().unwrap();
//!
//! let mut vm = Vm::new();
//! assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(20)));
//! ```

mod error;
mod opcode;
mod program;
mod value;
mod vm;

pub use error::VmError;
pub use opcode::{OpCode, CONST_INDEX_WIDTH, JUMP_TARGET_WIDTH, LOCAL_SLOT_WIDTH, OPCODE_WIDTH};
pub use program::{BuildError, Instr, LabelId, Program, ProgramBuilder};
pub use value::Value;
pub use vm::{Vm, DEFAULT_STEP_LIMIT};
