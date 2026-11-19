// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "error.rs"
// ============================================================================

use std::fmt;

use gindema_bits::BitsError;

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
        }
    }
}

impl std::error::Error for VmError {}

impl From<BitsError> for VmError {
    fn from(e: BitsError) -> Self {
        Self::Bits(e)
    }
}
