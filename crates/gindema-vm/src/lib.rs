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
//! value types). A textual assembly syntax now exists ([`assemble`]) -- see
//! its own module docs for the syntax. [`decode_program`] decodes an
//! already-built program back into a structured instruction list --
//! `gindema-obfuscate`'s control-flow flattening pass is built on it.
//! [`interleave_fluff`] is state-dependent decoding's bytecode-format
//! mechanism -- the one piece of that pass that had to live here rather
//! than entirely in `gindema-obfuscate`, unlike honeypot injection or
//! flattening; see its own module docs for why.
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

mod asm;
mod decode;
mod error;
mod fluff;
mod opcode;
mod opcode_map;
mod program;
mod remap;
mod serialize;
mod value;
mod vm;

pub use asm::{assemble, AsmError};
pub use decode::{decode_program, resolve_jump_target, DecodedInstr, Operand};
pub use error::{SerializeError, VmError};
pub use fluff::interleave_fluff;
pub use opcode::{
    OpCode, CONST_INDEX_WIDTH, JUMP_TARGET_WIDTH, LOCAL_SLOT_WIDTH, OPCODE_COUNT, OPCODE_WIDTH,
};
pub use opcode_map::{OpcodeMap, OpcodeMapError};
pub use program::{BuildError, Instr, LabelId, Program, ProgramBuilder};
pub use remap::remap_program;
pub use value::Value;
pub use vm::{Vm, DEFAULT_STEP_LIMIT};
