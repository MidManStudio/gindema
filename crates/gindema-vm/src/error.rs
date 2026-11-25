// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "error.rs"
// ============================================================================

use std::fmt;

use gindema_bits::BitsError;

use crate::opcode_map::OpcodeMapError;

/// Everything that can go wrong running a [`crate::Program`] on a [`crate::Vm`].
#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    /// An instruction needed a value on the stack and found none (or, for
    /// [`crate::OpCode::Swap`], fewer than two).
    StackUnderflow,
    /// An operation got operand types it doesn't support (e.g. `Add` on a
    /// `Bool`, or comparing across a type it refuses to compare).
    TypeMismatch,
    /// Integer division or modulo by zero. (Float division by zero follows
    /// IEEE 754 — `inf`/`NaN` — rather than erroring.)
    DivisionByZero,
    /// An integer operation would have overflowed `i64`.
    ArithmeticOverflow,
    /// The bytecode stream contained a byte that isn't a valid opcode.
    InvalidOpcode(u8),
    /// `LoadConst` referenced a constant-pool index past the end of the pool.
    InvalidConstIndex(u16),
    /// `LoadLocal`/`StoreLocal` referenced a local slot past the declared
    /// local count.
    InvalidLocalSlot(u8),
    /// [`Vm::run`](crate::Vm::run) executed more instructions than its step
    /// limit allows — a safety net against a runaway unbounded loop, not an
    /// architectural limit on legitimate programs. See
    /// [`Vm::run_with_step_limit`](crate::Vm::run_with_step_limit).
    StepLimitExceeded,
    /// Decoding the bytecode stream itself failed — a malformed or truncated
    /// program, as opposed to a valid stream with semantically invalid
    /// operands (those get their own variants above).
    Bits(BitsError),
    /// A `Jump`/`JumpIfFalse` target (see [`crate::decode_program`]) didn't
    /// match any instruction's bit offset and wasn't exactly the program's
    /// own end either — not something [`crate::ProgramBuilder`] can
    /// produce, but a decoded instruction list could in principle come
    /// from anywhere. See [`crate::resolve_jump_target`].
    InvalidJumpTarget(u64),
    /// A transform that can grow a program's bit length (currently only
    /// [`crate::interleave_fluff`]) would have pushed it past the 32-bit
    /// jump-target range — the same limit
    /// [`crate::BuildError::ProgramTooLarge`] enforces at build time, hit
    /// here instead because the growth happens after building.
    ProgramTooLarge,
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StackUnderflow => write!(f, "stack underflow"),
            Self::TypeMismatch => write!(f, "operand type mismatch"),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::ArithmeticOverflow => write!(f, "arithmetic overflow"),
            Self::InvalidOpcode(b) => write!(f, "invalid opcode byte {b}"),
            Self::InvalidConstIndex(i) => write!(f, "invalid constant index {i}"),
            Self::InvalidLocalSlot(s) => write!(f, "invalid local slot {s}"),
            Self::StepLimitExceeded => write!(f, "exceeded the interpreter's step limit"),
            Self::Bits(e) => write!(f, "malformed bytecode stream: {e}"),
            Self::InvalidJumpTarget(t) => write!(f, "jump target {t} matches no instruction"),
            Self::ProgramTooLarge => write!(f, "program exceeds a 32-bit jump target range"),
        }
    }
}

impl std::error::Error for VmError {}

impl From<BitsError> for VmError {
    fn from(e: BitsError) -> Self {
        Self::Bits(e)
    }
}

/// Everything that can go wrong parsing a serialized [`crate::Program`]
/// back from bytes ([`crate::Program::from_bytes`]). Kept separate from
/// [`VmError`]: parsing a file and running an already-built program are
/// different concerns, and a caller that loads a program and then runs it
/// shouldn't have to tell a parse failure apart from a runtime one by
/// pattern-matching a single combined enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerializeError {
    /// The data didn't start with the expected 4-byte magic.
    InvalidMagic,
    /// The data declared a format version this crate doesn't know how to
    /// read.
    UnsupportedVersion(u16),
    /// The data ended before a complete program was read.
    UnexpectedEof,
    /// A constant's type tag byte wasn't one this crate defines.
    InvalidValueTag(u8),
    /// The `fluff_seed` presence byte wasn't `0` (absent) or `1` (present,
    /// followed by the 8-byte seed) — see `docs/gindema-vm.md`'s
    /// `serialize.rs` section for the exact layout.
    InvalidFluffFlag(u8),
    /// The stored opcode permutation was invalid (see [`OpcodeMapError`]) —
    /// implies the data was corrupted or hand-edited, since
    /// [`crate::Program::to_bytes`] never writes one that isn't valid.
    OpcodeMap(OpcodeMapError),
}

impl fmt::Display for SerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "not a gindema-vm program file (bad magic)"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported program file version {v}"),
            Self::UnexpectedEof => write!(f, "program file ended unexpectedly"),
            Self::InvalidValueTag(t) => write!(f, "invalid constant type tag {t}"),
            Self::InvalidFluffFlag(b) => write!(f, "invalid fluff-seed presence byte {b}"),
            Self::OpcodeMap(e) => write!(f, "invalid opcode table in program file: {e}"),
        }
    }
}

impl std::error::Error for SerializeError {}

impl From<OpcodeMapError> for SerializeError {
    fn from(e: OpcodeMapError) -> Self {
        Self::OpcodeMap(e)
    }
}
