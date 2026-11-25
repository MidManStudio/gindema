// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "vm.rs"
// ============================================================================

use gindema_bits::BitReader;

use crate::error::VmError;
use crate::fluff::skip_fluff;
use crate::opcode::{OpCode, CONST_INDEX_WIDTH, JUMP_TARGET_WIDTH, LOCAL_SLOT_WIDTH, OPCODE_WIDTH};
use crate::program::Program;
use crate::value::Value;

/// A safety net against a runaway unbounded loop in a malformed or
/// adversarial program — not an architectural limit on legitimate ones.
/// [`Vm::run`] uses this; call [`Vm::run_with_step_limit`] directly for a
/// different bound.
pub const DEFAULT_STEP_LIMIT: u64 = 10_000_000;

/// The gindema-vm interpreter: a stack machine that decodes and executes a
/// [`Program`]'s bit-packed instruction stream one instruction at a time.
///
/// Holds only the value stack between runs — a fresh local-slot array is
/// allocated per [`Vm::run`] call, sized from the `Program` being run, so one
/// `Vm` can run multiple (unrelated) programs in sequence.
#[derive(Debug, Default)]
pub struct Vm {
    stack: Vec<Value>,
}

impl Vm {
    /// Creates a `Vm` with an empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current stack contents — exposed mainly for tests and debugging
    /// tools; a running program only ever sees its own top of stack.
    pub fn stack(&self) -> &[Value] {
        &self.stack
    }

    /// Runs `program` to completion (a `Halt` instruction, or the stream
    /// running out) and returns whatever is left on top of the stack, if
    /// anything. Uses [`DEFAULT_STEP_LIMIT`] as a runaway-loop safety net —
    /// see [`Vm::run_with_step_limit`] to change that.
    pub fn run(&mut self, program: &Program) -> Result<Option<Value>, VmError> {
        self.run_with_step_limit(program, DEFAULT_STEP_LIMIT)
    }

    /// Same as [`Vm::run`], with an explicit cap on the number of
    /// instructions executed before giving up with
    /// [`VmError::StepLimitExceeded`].
    ///
    /// Transparently skips state-dependent fluff bits if `program` carries
    /// any (see `docs/gindema-vm.md`'s `fluff.rs` section) — nothing about
    /// running a fluffed program looks different from the caller's side.
    pub fn run_with_step_limit(
        &mut self,
        program: &Program,
        step_limit: u64,
    ) -> Result<Option<Value>, VmError> {
        let mut locals = vec![Value::Int(0); program.local_count() as usize];
        let mut reader = BitReader::new(&program.code);
        let mut steps: u64 = 0;

        loop {
            if steps >= step_limit {
                return Err(VmError::StepLimitExceeded);
            }
            steps += 1;

            skip_fluff(&mut reader, program)?;
            let opcode_byte = reader.read_bits(OPCODE_WIDTH)? as u8;
            let opcode = program
                .opcode_map()
                .decode(opcode_byte)
                .ok_or(VmError::InvalidOpcode(opcode_byte))?;

            match opcode {
                OpCode::Halt => break,
                OpCode::Pop => {
                    self.pop()?;
                }
                OpCode::Dup => {
                    let top = *self.stack.last().ok_or(VmError::StackUnderflow)?;
                    self.stack.push(top);
                }
                OpCode::Swap => {
                    let len = self.stack.len();
                    if len < 2 {
                        return Err(VmError::StackUnderflow);
                    }
                    self.stack.swap(len - 1, len - 2);
                }
                OpCode::Add => self.binary(Value::try_add)?,
                OpCode::Sub => self.binary(Value::try_sub)?,
                OpCode::Mul => self.binary(Value::try_mul)?,
                OpCode::Div => self.binary(Value::try_div)?,
                OpCode::Neg => {
                    let a = self.pop()?;
                    self.stack.push(a.try_neg()?);
                }
                OpCode::Not => {
                    let a = self.pop()?;
                    self.stack.push(a.try_not()?);
                }
                OpCode::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.stack.push(a.eq_value(b));
                }
                OpCode::Lt => self.binary(Value::try_lt)?,
                OpCode::LoadConst => {
                    let idx = reader.read_bits(CONST_INDEX_WIDTH)? as u16;
                    let value = *program
                        .constants()
                        .get(idx as usize)
                        .ok_or(VmError::InvalidConstIndex(idx))?;
                    self.stack.push(value);
                }
                OpCode::LoadLocal => {
                    let slot = reader.read_bits(LOCAL_SLOT_WIDTH)? as u8;
                    let value = *locals
                        .get(slot as usize)
                        .ok_or(VmError::InvalidLocalSlot(slot))?;
                    self.stack.push(value);
                }
                OpCode::StoreLocal => {
                    let slot = reader.read_bits(LOCAL_SLOT_WIDTH)? as u8;
                    let value = self.pop()?;
                    let dst = locals
                        .get_mut(slot as usize)
                        .ok_or(VmError::InvalidLocalSlot(slot))?;
                    *dst = value;
                }
                OpCode::Jump => {
                    let target = reader.read_bits(JUMP_TARGET_WIDTH)?;
                    reader.seek_to_bit(target)?;
                }
                OpCode::JumpIfFalse => {
                    let target = reader.read_bits(JUMP_TARGET_WIDTH)?;
                    let cond = self.pop()?;
                    let cond = cond.as_bool().ok_or(VmError::TypeMismatch)?;
                    if !cond {
                        reader.seek_to_bit(target)?;
                    }
                }
            }
        }

        Ok(self.stack.pop())
    }

    fn pop(&mut self) -> Result<Value, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow)
    }

    /// Pops `b` then `a` and pushes `f(a, b)` — the shared shape of every
    /// binary operator.
    fn binary(
        &mut self,
        f: impl FnOnce(Value, Value) -> Result<Value, VmError>,
    ) -> Result<(), VmError> {
        let b = self.pop()?;
        let a = self.pop()?;
        self.stack.push(f(a, b)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::program::ProgramBuilder;

    #[test]
    fn arithmetic_leaves_the_result_on_top() {
        // (2 + 3) * 4 == 20
        let mut b = ProgramBuilder::new(0);
        let two = b.add_const(Value::Int(2)).unwrap();
        let three = b.add_const(Value::Int(3)).unwrap();
        let four = b.add_const(Value::Int(4)).unwrap();
        b.load_const(two)
            .load_const(three)
            .add()
            .load_const(four)
            .mul()
            .halt();
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(20)));
    }

    #[test]
    fn locals_round_trip_through_store_and_load() {
        let mut b = ProgramBuilder::new(1);
        let five = b.add_const(Value::Int(5)).unwrap();
        b.load_const(five)
            .store_local(0)
            .load_local(0)
            .load_local(0)
            .add()
            .halt();
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(10)));
    }

    #[test]
    fn forward_jump_skips_the_dead_branch() {
        // if (3 == 4) { push 1 } else { push 2 } -- condition is false, so
        // the else branch's value (2) is what's left on the stack.
        let mut b = ProgramBuilder::new(0);
        let else_branch = b.new_label();
        let end = b.new_label();
        let one = b.add_const(Value::Int(1)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();
        let three = b.add_const(Value::Int(3)).unwrap();
        let four = b.add_const(Value::Int(4)).unwrap();

        b.load_const(three).load_const(four).eq();
        b.jump_if_false(else_branch);
        b.load_const(one);
        b.jump(end);
        b.place_label(else_branch);
        b.load_const(two);
        b.place_label(end);
        b.halt();
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(2)));
    }

    #[test]
    fn backward_jump_implements_a_counting_loop() {
        // local[0] = 0; while local[0] < 5 { local[0] = local[0] + 1 }; return local[0]
        let mut b = ProgramBuilder::new(1);
        let zero = b.add_const(Value::Int(0)).unwrap();
        let one = b.add_const(Value::Int(1)).unwrap();
        let five = b.add_const(Value::Int(5)).unwrap();

        b.load_const(zero).store_local(0);
        let loop_start = b.new_label();
        let loop_end = b.new_label();
        b.place_label(loop_start);
        b.load_local(0).load_const(five).lt();
        b.jump_if_false(loop_end);
        b.load_local(0).load_const(one).add().store_local(0);
        b.jump(loop_start);
        b.place_label(loop_end);
        b.load_local(0);
        b.halt();
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&program).unwrap(), Some(Value::Int(5)));
    }

    #[test]
    fn stack_underflow_is_reported_not_panicked() {
        let program = ProgramBuilder::new(0).add().halt().build().unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program), Err(VmError::StackUnderflow));
    }

    #[test]
    fn an_infinite_loop_hits_the_step_limit_instead_of_hanging() {
        let mut b = ProgramBuilder::new(0);
        let top = b.new_label();
        b.place_label(top);
        b.jump(top);
        let program = b.build().unwrap();

        let mut vm = Vm::new();
        assert_eq!(
            vm.run_with_step_limit(&program, 1_000),
            Err(VmError::StepLimitExceeded)
        );
    }

    #[test]
    fn invalid_const_and_local_indices_are_reported() {
        let mut b = ProgramBuilder::new(0);
        b.load_const(0).halt();
        let program = b.build().unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program), Err(VmError::InvalidConstIndex(0)));

        let mut b = ProgramBuilder::new(0);
        b.load_local(0).halt();
        let program = b.build().unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&program), Err(VmError::InvalidLocalSlot(0)));
    }
}
