// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "opcode.rs"
// ============================================================================

/// Width, in bits, of the opcode field at the start of every instruction.
/// 6 bits allows 64 distinct opcodes; 17 are defined so far (see [`OpCode`]).
pub const OPCODE_WIDTH: u32 = 6;
/// Width, in bits, of `LoadConst`'s constant-pool-index operand.
pub const CONST_INDEX_WIDTH: u32 = 16;
/// Width, in bits, of `LoadLocal`/`StoreLocal`'s local-slot-index operand.
pub const LOCAL_SLOT_WIDTH: u32 = 8;
/// Width, in bits, of `Jump`/`JumpIfFalse`'s target operand — an absolute bit
/// offset into the program's own bytecode stream.
pub const JUMP_TARGET_WIDTH: u32 = 32;
/// The number of opcodes [`OpCode`] defines — the length of [`OpCode::ALL`].
pub const OPCODE_COUNT: usize = 17;

/// gindema-vm's own instruction set. Invented for this project specifically,
/// not modeled on any real bytecode VM's numbering or mnemonics — see
/// `docs/gindema-vm.md` for why that matters here. Per-build opcode
/// remapping (so two builds of the same program don't share numbering) is
/// `gindema-obfuscate`'s job, not this crate's; this is the one fixed
/// baseline instruction set that remapping would scramble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OpCode {
    /// Stops execution. The interpreter returns whatever is left on top of
    /// the stack, if anything.
    Halt = 0,
    /// Pops and discards the top of the stack.
    Pop = 1,
    /// Duplicates the top of the stack.
    Dup = 2,
    /// Swaps the top two stack values.
    Swap = 3,
    /// Pops `b`, then `a`; pushes `a + b`.
    Add = 4,
    /// Pops `b`, then `a`; pushes `a - b`.
    Sub = 5,
    /// Pops `b`, then `a`; pushes `a * b`.
    Mul = 6,
    /// Pops `b`, then `a`; pushes `a / b`.
    Div = 7,
    /// Pops `a`; pushes `-a`.
    Neg = 8,
    /// Pops `a` (must be `Bool`); pushes `!a`.
    Not = 9,
    /// Pops `b`, then `a`; pushes `a == b`.
    Eq = 10,
    /// Pops `b`, then `a`; pushes `a < b`.
    Lt = 11,
    /// Pushes a copy of constant-pool entry `operand` (16-bit index).
    LoadConst = 12,
    /// Pushes a copy of local slot `operand` (8-bit index).
    LoadLocal = 13,
    /// Pops the top of the stack into local slot `operand` (8-bit index).
    StoreLocal = 14,
    /// Unconditionally seeks to the absolute bit offset `operand` (32-bit).
    Jump = 15,
    /// Pops `a` (must be `Bool`); if `a` is false, seeks to the absolute bit
    /// offset `operand` (32-bit). Otherwise falls through.
    JumpIfFalse = 16,
}

impl OpCode {
    /// Every defined opcode, in the same order as their canonical numeric
    /// values (`Halt` first, `JumpIfFalse` last). [`crate::OpcodeMap`]'s
    /// identity and permutation builders iterate this list rather than the
    /// raw `0..=16` range, so a future opcode addition only has to update
    /// this one array.
    pub const ALL: [Self; OPCODE_COUNT] = [
        Self::Halt,
        Self::Pop,
        Self::Dup,
        Self::Swap,
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::Div,
        Self::Neg,
        Self::Not,
        Self::Eq,
        Self::Lt,
        Self::LoadConst,
        Self::LoadLocal,
        Self::StoreLocal,
        Self::Jump,
        Self::JumpIfFalse,
    ];

    /// The numeric encoding written to the bytecode stream.
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a numeric opcode value, or `None` if it isn't one of the
    /// defined opcodes.
    pub const fn from_u8(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::Halt,
            1 => Self::Pop,
            2 => Self::Dup,
            3 => Self::Swap,
            4 => Self::Add,
            5 => Self::Sub,
            6 => Self::Mul,
            7 => Self::Div,
            8 => Self::Neg,
            9 => Self::Not,
            10 => Self::Eq,
            11 => Self::Lt,
            12 => Self::LoadConst,
            13 => Self::LoadLocal,
            14 => Self::StoreLocal,
            15 => Self::Jump,
            16 => Self::JumpIfFalse,
            _ => return None,
        })
    }

    /// Width, in bits, of this opcode's operand — `0` if it takes none.
    pub const fn operand_width(self) -> u32 {
        match self {
            Self::LoadConst => CONST_INDEX_WIDTH,
            Self::LoadLocal | Self::StoreLocal => LOCAL_SLOT_WIDTH,
            Self::Jump | Self::JumpIfFalse => JUMP_TARGET_WIDTH,
            Self::Halt
            | Self::Pop
            | Self::Dup
            | Self::Swap
            | Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Neg
            | Self::Not
            | Self::Eq
            | Self::Lt => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_defined_opcode_round_trips_through_its_byte() {
        for byte in 0..=16u8 {
            let op = OpCode::from_u8(byte).unwrap_or_else(|| panic!("opcode {byte} should decode"));
            assert_eq!(op.to_u8(), byte);
        }
    }

    #[test]
    fn an_undefined_byte_does_not_decode() {
        assert_eq!(OpCode::from_u8(17), None);
        assert_eq!(OpCode::from_u8(63), None);
        assert_eq!(OpCode::from_u8(255), None);
    }

    #[test]
    fn every_opcode_fits_the_opcode_width() {
        for byte in 0..=16u8 {
            assert!((byte as u32) < (1 << OPCODE_WIDTH));
        }
    }

    #[test]
    fn all_contains_every_decodable_opcode_exactly_once() {
        assert_eq!(OpCode::ALL.len(), OPCODE_COUNT);
        for byte in 0..=16u8 {
            let op = OpCode::from_u8(byte).unwrap();
            assert_eq!(
                OpCode::ALL.iter().filter(|o| **o == op).count(),
                1,
                "opcode {byte} should appear in ALL exactly once"
            );
        }
    }

    #[test]
    fn operand_widths_match_the_declared_constants() {
        assert_eq!(OpCode::Halt.operand_width(), 0);
        assert_eq!(OpCode::LoadConst.operand_width(), CONST_INDEX_WIDTH);
        assert_eq!(OpCode::LoadLocal.operand_width(), LOCAL_SLOT_WIDTH);
        assert_eq!(OpCode::StoreLocal.operand_width(), LOCAL_SLOT_WIDTH);
        assert_eq!(OpCode::Jump.operand_width(), JUMP_TARGET_WIDTH);
        assert_eq!(OpCode::JumpIfFalse.operand_width(), JUMP_TARGET_WIDTH);
    }
}
