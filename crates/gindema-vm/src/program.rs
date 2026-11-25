// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "program.rs"
// ============================================================================

use std::fmt;

use gindema_bits::BitWriter;

use crate::opcode::OpCode;
use crate::opcode_map::OpcodeMap;
use crate::value::Value;

/// An opaque handle to a not-yet-resolved jump target, created by
/// [`ProgramBuilder::new_label`] and fixed to a position by
/// [`ProgramBuilder::place_label`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelId(usize);

/// One instruction, in the builder's intermediate form — before bit-packing,
/// jump targets are still [`LabelId`]s rather than resolved bit offsets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instr {
    /// See [`OpCode::Halt`].
    Halt,
    /// See [`OpCode::Pop`].
    Pop,
    /// See [`OpCode::Dup`].
    Dup,
    /// See [`OpCode::Swap`].
    Swap,
    /// See [`OpCode::Add`].
    Add,
    /// See [`OpCode::Sub`].
    Sub,
    /// See [`OpCode::Mul`].
    Mul,
    /// See [`OpCode::Div`].
    Div,
    /// See [`OpCode::Neg`].
    Neg,
    /// See [`OpCode::Not`].
    Not,
    /// See [`OpCode::Eq`].
    Eq,
    /// See [`OpCode::Lt`].
    Lt,
    /// See [`OpCode::LoadConst`]. The operand is a constant-pool index
    /// returned by [`ProgramBuilder::add_const`].
    LoadConst(u16),
    /// See [`OpCode::LoadLocal`]. The operand is a local slot index.
    LoadLocal(u8),
    /// See [`OpCode::StoreLocal`]. The operand is a local slot index.
    StoreLocal(u8),
    /// See [`OpCode::Jump`]. The operand is a label created by
    /// [`ProgramBuilder::new_label`].
    Jump(LabelId),
    /// See [`OpCode::JumpIfFalse`]. The operand is a label created by
    /// [`ProgramBuilder::new_label`].
    JumpIfFalse(LabelId),
}

impl Instr {
    const fn opcode(self) -> OpCode {
        match self {
            Self::Halt => OpCode::Halt,
            Self::Pop => OpCode::Pop,
            Self::Dup => OpCode::Dup,
            Self::Swap => OpCode::Swap,
            Self::Add => OpCode::Add,
            Self::Sub => OpCode::Sub,
            Self::Mul => OpCode::Mul,
            Self::Div => OpCode::Div,
            Self::Neg => OpCode::Neg,
            Self::Not => OpCode::Not,
            Self::Eq => OpCode::Eq,
            Self::Lt => OpCode::Lt,
            Self::LoadConst(_) => OpCode::LoadConst,
            Self::LoadLocal(_) => OpCode::LoadLocal,
            Self::StoreLocal(_) => OpCode::StoreLocal,
            Self::Jump(_) => OpCode::Jump,
            Self::JumpIfFalse(_) => OpCode::JumpIfFalse,
        }
    }

    /// The operand to encode, given each label's already-resolved bit offset
    /// (indexed by [`LabelId`]). `None` for instructions with no operand.
    fn operand_value(self, resolved_labels: &[u64]) -> Option<u64> {
        match self {
            Self::LoadConst(idx) => Some(u64::from(idx)),
            Self::LoadLocal(slot) | Self::StoreLocal(slot) => Some(u64::from(slot)),
            Self::Jump(LabelId(l)) | Self::JumpIfFalse(LabelId(l)) => Some(resolved_labels[l]),
            _ => None,
        }
    }
}

/// Everything that can go wrong assembling a [`ProgramBuilder`] into a
/// [`Program`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// A label was created with [`ProgramBuilder::new_label`] but never fixed
    /// to a position with [`ProgramBuilder::place_label`] before [`build`
    /// was called](ProgramBuilder::build).
    UnresolvedLabel(LabelId),
    /// More than `u16::MAX + 1` constants were added — the constant pool
    /// index operand is 16 bits wide.
    TooManyConstants,
    /// The encoded bytecode stream would be longer than `u32::MAX` bits — the
    /// jump-target operand is 32 bits wide.
    ProgramTooLarge,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnresolvedLabel(LabelId(id)) => {
                write!(f, "label {id} was created but never placed")
            }
            Self::TooManyConstants => write!(f, "too many constants for a 16-bit index"),
            Self::ProgramTooLarge => write!(f, "program exceeds a 32-bit jump target range"),
        }
    }
}

impl std::error::Error for BuildError {}

/// A fully assembled gindema-vm program: a bit-packed instruction stream plus
/// the constant pool and local-slot count it refers into. Build one with
/// [`ProgramBuilder`]; run one with [`crate::Vm::run`].
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub(crate) code: Vec<u8>,
    /// The exact bit length of real instruction content in `code`, as
    /// opposed to `code.len() * 8`, which also counts
    /// [`gindema_bits::BitWriter::into_bytes`]'s trailing zero-padding up to
    /// the next byte boundary. See [`Program::code_bit_len`] for why this
    /// is tracked explicitly instead of inferred.
    pub(crate) code_bit_len: u64,
    pub(crate) constants: Vec<Value>,
    pub(crate) local_count: u8,
    pub(crate) opcode_map: OpcodeMap,
    /// `Some(seed)` if this program's stream has been interleaved with
    /// state-dependent fluff bits (see [`crate::interleave_fluff`]/
    /// `docs/gindema-vm.md`'s `fluff.rs` section); `None` otherwise.
    /// [`ProgramBuilder::build`] always produces `None`, the same way it
    /// always produces [`OpcodeMap::identity`].
    pub(crate) fluff_seed: Option<u64>,
}

impl Program {
    /// The constant pool, in index order.
    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    /// The number of local slots this program declared.
    pub fn local_count(&self) -> u8 {
        self.local_count
    }

    /// The length of the encoded bytecode stream, in bytes (after
    /// byte-alignment padding).
    pub fn code_len_bytes(&self) -> usize {
        self.code.len()
    }

    /// The exact bit length of this program's real instruction content —
    /// `code_bytes().len() * 8` minus whatever trailing zero-padding bits
    /// [`gindema_bits::BitWriter::into_bytes`] added to reach a byte
    /// boundary.
    ///
    /// A pass that walks every instruction front-to-back without following
    /// control flow (like [`crate::remap_program`]) needs this to know
    /// exactly where real content ends: stopping at the first `Halt`
    /// opcode is wrong once dead code (a `gindema-obfuscate` honeypot, for
    /// instance) can contain its own `Halt` earlier in the stream than the
    /// program's real one, and stopping only when no bits remain risks
    /// misreading leftover padding bits as a phantom instruction. Decoding
    /// up to exactly this many bits avoids both.
    pub fn code_bit_len(&self) -> u64 {
        self.code_bit_len
    }

    /// The raw, bit-packed instruction stream. Exposed for passes that
    /// transform an already-built program (such as
    /// [`crate::remap_program`]) — running a program never needs this
    /// directly; [`crate::Vm::run`] reads it internally.
    pub fn code_bytes(&self) -> &[u8] {
        &self.code
    }

    /// The [`OpcodeMap`] this program's instruction stream was encoded
    /// with. [`crate::ProgramBuilder::build`] always produces
    /// [`OpcodeMap::identity`]; a program that's been through
    /// [`crate::remap_program`] carries whatever map it was remapped to.
    pub fn opcode_map(&self) -> &OpcodeMap {
        &self.opcode_map
    }

    /// The seed this program's stream was interleaved with state-dependent
    /// fluff bits under, if it has been — see [`crate::interleave_fluff`].
    /// `None` for a program fresh out of [`ProgramBuilder::build`], or one
    /// that's only been through [`crate::remap_program`] with no fluff pass
    /// involved at all.
    pub fn fluff_seed(&self) -> Option<u64> {
        self.fluff_seed
    }
}

/// Builds a [`Program`] by emitting instructions in order and resolving
/// jump targets via [`LabelId`]s, rather than requiring the caller to know
/// bit offsets by hand.
///
/// Label resolution is two-pass rather than back-patched into the bit
/// stream: every instruction's encoded width is fixed by its opcode (see
/// [`OpCode::operand_width`]), so a forward pass over the instruction list
/// alone — before any bits are written — already determines every
/// instruction's final bit offset. [`ProgramBuilder::build`] uses that pass
/// to resolve every label, then encodes the whole stream in a second pass
/// with every jump target already known. This sidesteps needing to seek
/// backward and overwrite already-written bits in
/// [`gindema_bits::BitWriter`], which doesn't support that.
///
/// There is no assembly syntax yet — a program is built by calling emitter
/// methods directly, which is enough to drive and test the interpreter.
pub struct ProgramBuilder {
    instrs: Vec<Instr>,
    /// Instruction index each label was placed at, or `None` if not yet
    /// placed. Resolved to a bit offset only in `build`.
    label_targets: Vec<Option<usize>>,
    constants: Vec<Value>,
    local_count: u8,
}

impl ProgramBuilder {
    /// Starts a new, empty program with `local_count` local slots (all
    /// initialized to `Value::Int(0)` when the program runs).
    pub fn new(local_count: u8) -> Self {
        Self {
            instrs: Vec::new(),
            label_targets: Vec::new(),
            constants: Vec::new(),
            local_count,
        }
    }

    /// Creates a new, as-yet-unplaced label.
    pub fn new_label(&mut self) -> LabelId {
        self.label_targets.push(None);
        LabelId(self.label_targets.len() - 1)
    }

    /// Fixes `label` to point at the next instruction emitted after this
    /// call (or, if nothing more is emitted, to the end of the program).
    pub fn place_label(&mut self, label: LabelId) -> &mut Self {
        self.label_targets[label.0] = Some(self.instrs.len());
        self
    }

    /// Adds a value to the constant pool and returns its index for use with
    /// [`ProgramBuilder::load_const`].
    ///
    /// # Errors
    /// [`BuildError::TooManyConstants`] past the 16-bit index space.
    pub fn add_const(&mut self, value: Value) -> Result<u16, BuildError> {
        let idx = self.constants.len();
        if idx > u16::MAX as usize {
            return Err(BuildError::TooManyConstants);
        }
        self.constants.push(value);
        Ok(idx as u16)
    }

    /// Appends one instruction. The convenience methods below (`halt`,
    /// `add`, `load_const`, ...) are shorthand for this.
    pub fn emit(&mut self, instr: Instr) -> &mut Self {
        self.instrs.push(instr);
        self
    }

    /// See [`OpCode::Halt`].
    pub fn halt(&mut self) -> &mut Self {
        self.emit(Instr::Halt)
    }
    /// See [`OpCode::Pop`].
    pub fn pop(&mut self) -> &mut Self {
        self.emit(Instr::Pop)
    }
    /// See [`OpCode::Dup`].
    pub fn dup(&mut self) -> &mut Self {
        self.emit(Instr::Dup)
    }
    /// See [`OpCode::Swap`].
    pub fn swap(&mut self) -> &mut Self {
        self.emit(Instr::Swap)
    }
    /// See [`OpCode::Add`].
    pub fn add(&mut self) -> &mut Self {
        self.emit(Instr::Add)
    }
    /// See [`OpCode::Sub`].
    pub fn sub(&mut self) -> &mut Self {
        self.emit(Instr::Sub)
    }
    /// See [`OpCode::Mul`].
    pub fn mul(&mut self) -> &mut Self {
        self.emit(Instr::Mul)
    }
    /// See [`OpCode::Div`].
    pub fn div(&mut self) -> &mut Self {
        self.emit(Instr::Div)
    }
    /// See [`OpCode::Neg`].
    pub fn neg(&mut self) -> &mut Self {
        self.emit(Instr::Neg)
    }
    /// See [`OpCode::Not`].
    pub fn not(&mut self) -> &mut Self {
        self.emit(Instr::Not)
    }
    /// See [`OpCode::Eq`].
    pub fn eq(&mut self) -> &mut Self {
        self.emit(Instr::Eq)
    }
    /// See [`OpCode::Lt`].
    pub fn lt(&mut self) -> &mut Self {
        self.emit(Instr::Lt)
    }
    /// See [`OpCode::LoadConst`].
    pub fn load_const(&mut self, index: u16) -> &mut Self {
        self.emit(Instr::LoadConst(index))
    }
    /// See [`OpCode::LoadLocal`].
    pub fn load_local(&mut self, slot: u8) -> &mut Self {
        self.emit(Instr::LoadLocal(slot))
    }
    /// See [`OpCode::StoreLocal`].
    pub fn store_local(&mut self, slot: u8) -> &mut Self {
        self.emit(Instr::StoreLocal(slot))
    }
    /// See [`OpCode::Jump`].
    pub fn jump(&mut self, target: LabelId) -> &mut Self {
        self.emit(Instr::Jump(target))
    }
    /// See [`OpCode::JumpIfFalse`].
    pub fn jump_if_false(&mut self, target: LabelId) -> &mut Self {
        self.emit(Instr::JumpIfFalse(target))
    }

    /// Resolves every label and bit-packs the instruction stream into a
    /// [`Program`].
    ///
    /// # Errors
    /// [`BuildError::UnresolvedLabel`] if any label created with
    /// [`ProgramBuilder::new_label`] was never placed.
    /// [`BuildError::ProgramTooLarge`] if the encoded stream would exceed a
    /// 32-bit bit-offset range.
    ///
    /// Takes `&mut self` rather than `self` so it composes with the chained
    /// `&mut Self`-returning emitter methods above — e.g.
    /// `ProgramBuilder::new(0).halt().build()`. Internally it takes the
    /// instruction list, label table, and constant pool out of `self` (via
    /// [`std::mem::take`]) rather than cloning them; calling `build` again on
    /// the same builder afterward produces an empty, zero-instruction
    /// program rather than an error, which is harmless but not useful.
    pub fn build(&mut self) -> Result<Program, BuildError> {
        let instrs = std::mem::take(&mut self.instrs);
        let label_targets = std::mem::take(&mut self.label_targets);
        let constants = std::mem::take(&mut self.constants);

        // Pass 1: every instruction's encoded width is fixed by its opcode
        // alone, so the bit offset of every instruction (and of the position
        // just past the last one, for a label placed at the very end) is
        // known without writing a single bit yet.
        let mut offsets = Vec::with_capacity(instrs.len() + 1);
        let mut bit: u64 = 0;
        for instr in &instrs {
            offsets.push(bit);
            bit += u64::from(OpCode::operand_width(instr.opcode()))
                + u64::from(crate::opcode::OPCODE_WIDTH);
        }
        offsets.push(bit);
        let total_bits = bit;
        if total_bits > u64::from(u32::MAX) {
            return Err(BuildError::ProgramTooLarge);
        }

        let mut resolved_labels = Vec::with_capacity(label_targets.len());
        for (id, target) in label_targets.iter().enumerate() {
            match target {
                Some(instr_idx) => resolved_labels.push(offsets[*instr_idx]),
                None => return Err(BuildError::UnresolvedLabel(LabelId(id))),
            }
        }

        // Pass 2: encode, with every jump target already a known bit offset.
        let mut writer = BitWriter::new();
        for instr in &instrs {
            writer
                .write_bits(
                    u64::from(instr.opcode().to_u8()),
                    crate::opcode::OPCODE_WIDTH,
                )
                .expect("an opcode byte always fits in OPCODE_WIDTH bits");
            if let Some(value) = instr.operand_value(&resolved_labels) {
                let width = instr.opcode().operand_width();
                writer.write_bits(value, width).expect(
                    "operand values are bounds-checked when built (constant/local indices) \
                     or bounded by the ProgramTooLarge check above (jump targets)",
                );
            }
        }

        Ok(Program {
            code: writer.into_bytes(),
            code_bit_len: total_bits,
            constants,
            local_count: self.local_count,
            opcode_map: OpcodeMap::identity(),
            fluff_seed: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_program_is_just_a_halt_opcode() {
        let program = ProgramBuilder::new(0).halt().build().unwrap();
        // One opcode (6 bits), byte-aligned up to 8.
        assert_eq!(program.code_len_bytes(), 1);
    }

    #[test]
    fn unplaced_label_fails_to_build() {
        let mut b = ProgramBuilder::new(0);
        let label = b.new_label();
        b.jump(label).halt();
        assert_eq!(b.build(), Err(BuildError::UnresolvedLabel(label)));
    }

    #[test]
    fn label_placed_at_the_end_resolves_to_the_programs_end() {
        let mut b = ProgramBuilder::new(0);
        let end = b.new_label();
        b.jump(end);
        b.place_label(end);
        // Nothing emitted after placing `end` — it should still resolve
        // (to the bit offset right after the jump instruction) rather than
        // erroring.
        assert!(b.build().is_ok());
    }

    #[test]
    fn add_const_rejects_more_than_a_16_bit_index_space() {
        let mut b = ProgramBuilder::new(0);
        for _ in 0..=u16::MAX as u32 {
            b.add_const(Value::Int(0)).unwrap();
        }
        assert_eq!(
            b.add_const(Value::Int(0)),
            Err(BuildError::TooManyConstants)
        );
    }
}
