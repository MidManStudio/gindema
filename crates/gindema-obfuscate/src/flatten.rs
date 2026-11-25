// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "flatten.rs"
// ============================================================================

use gindema_vm::{
    decode_program, resolve_jump_target, DecodedInstr, LabelId, OpCode, Operand, Program,
    ProgramBuilder,
};

use gindema_common::SplitMix64;

use crate::opcode_remap::ObfuscateError;

/// Where control goes once a block finishes: either another block (by its
/// index into this pass's own block list, not the original bit offset
/// anymore), or nowhere further — the program's real end. `None` is the
/// same "jump past the very last instruction" case
/// [`gindema_vm::resolve_jump_target`] already recognizes: a label placed
/// at a program's end with nothing emitted after it.
type Successor = Option<usize>;

/// How a block hands off to whatever comes after it, in terms of the
/// *original* program's own control transfer at that point.
enum Terminator {
    /// The block ended in `Halt`.
    Halt,
    /// The block ended in `Jump`, or in an ordinary instruction that simply
    /// falls into the next block (both are "always go here next").
    Goto(Successor),
    /// The block ended in `JumpIfFalse`: `on_false` if the popped condition
    /// was `false`, `on_true` (always the block's own fallthrough) if it
    /// was `true`.
    Branch {
        on_false: Successor,
        on_true: Successor,
    },
}

struct Block {
    /// Every instruction in this block except its terminator (`Halt`,
    /// `Jump`, and `JumpIfFalse` are never re-emitted verbatim — see
    /// [`Terminator`]).
    body: Vec<DecodedInstr>,
    terminator: Terminator,
}

/// Flattens `program`'s control flow into a single dispatcher loop: every
/// block becomes a case keyed on a new "state" local, tested in a flat
/// ladder of equality checks, with every real control transfer — including
/// an ordinary block's own fallthrough into the next one — now reading as
/// "set state, jump back to the dispatcher" instead of a direct jump to
/// wherever that destination happens to sit in the stream. A tool that
/// recovers a control-flow graph by reading jump targets sees one loop with
/// one dispatch point and no other edges between blocks at all, regardless
/// of whether `program`'s real shape was a single `if`, deeply nested
/// branches, or the loop in this module's own doctest below.
///
/// Built on [`decode_program`] — flattening doesn't care whether `program`
/// already carries fluff or a non-identity opcode map; `decode_program` is
/// transparent to both. The *output* always has neither: it's a fresh
/// [`ProgramBuilder::build`], so apply `gindema_vm::interleave_fluff` and
/// `crate::remap_opcodes` *after* flattening in a pipeline, not before —
/// flattening would otherwise just decode them away and rebuild without
/// them, silently discarding whatever they added.
///
/// # How a block's own fallthrough becomes a real transfer
/// Basic-block leaders are found the standard way: bit `0`; every
/// `Jump`/`JumpIfFalse` target; and the bit offset right after every
/// `Jump`/`JumpIfFalse` (its conditional fallthrough, or — for `Jump` — the
/// position any following instruction would start decoding dead, unreached
/// code at, which still needs its own block the same way a
/// `gindema-obfuscate` honeypot's dead end does). A block whose last real
/// instruction is ordinary (not a control transfer) simply continues into
/// whatever block starts right after it — flattening makes that an
/// explicit `Goto` like any other, so it goes through the dispatcher too,
/// rather than leaving one real fallthrough edge sitting outside the
/// flattened structure as a tell.
///
/// # The one place a direct branch still remains
/// `JumpIfFalse` is the only conditional primitive this instruction set
/// has, and there's no select/ternary opcode to pick between two "which
/// state value to set" constants without one — see
/// `docs/gindema-obfuscate.md`'s "Known limits" for the residual local
/// branch this produces and exactly how much (or little) of the original
/// shape it still reveals.
///
/// # Errors
/// Propagates [`gindema_vm::VmError`] (via [`ObfuscateError::Vm`]) from
/// `decode_program` or an internal [`resolve_jump_target`] call if
/// `program`'s stream is malformed, and from `build()` if flattening
/// somehow produced an unresolved label (not something this function's own
/// logic can do, but not asserted away either).
/// [`ObfuscateError::TooManyLocals`] if `program` already declares all 255
/// local slots a `u8` count allows — flattening needs one more, for the
/// dispatcher's own state variable.
pub fn flatten_control_flow(program: &Program, seed: u64) -> Result<Program, ObfuscateError> {
    let instrs = decode_program(program)?;
    let end_bit = program.code_bit_len();

    if instrs.is_empty() {
        // Degenerate case: a program with no instructions at all has no
        // control flow to flatten. `ProgramBuilder::new(n).build()` with
        // nothing emitted produces exactly this (see `program.rs`'s own
        // `empty_program_is_just_a_halt_opcode` test for the ordinary,
        // non-degenerate sibling of this case, which does have one Halt).
        let mut b = ProgramBuilder::new(program.local_count());
        return Ok(b.build()?);
    }

    let new_local_count = program
        .local_count()
        .checked_add(1)
        .ok_or(ObfuscateError::TooManyLocals)?;

    // --- Leader discovery -------------------------------------------------
    let mut leader_set = std::collections::BTreeSet::new();
    leader_set.insert(0u64);
    for instr in &instrs {
        if matches!(instr.opcode, OpCode::Jump | OpCode::JumpIfFalse) {
            let Operand::JumpTarget(target) = instr.operand else {
                unreachable!("decode_program always pairs Jump/JumpIfFalse with JumpTarget");
            };
            if target < end_bit {
                leader_set.insert(target);
            }
            let after = instr.bit_offset + instr.bit_len;
            if after < end_bit {
                leader_set.insert(after);
            }
        }
    }
    let leaders: Vec<u64> = leader_set.into_iter().collect();

    // --- Partition instructions into blocks, and record which block each
    // instruction index belongs to (needed to turn a resolved instruction
    // index, from `resolve_jump_target`, into a block index). ------------
    let mut block_bodies: Vec<Vec<DecodedInstr>> = vec![Vec::new(); leaders.len()];
    let mut block_of_instr: Vec<usize> = Vec::with_capacity(instrs.len());
    let mut leader_idx = 0;
    for instr in &instrs {
        while leader_idx + 1 < leaders.len() && instr.bit_offset >= leaders[leader_idx + 1] {
            leader_idx += 1;
        }
        block_of_instr.push(leader_idx);
        block_bodies[leader_idx].push(*instr);
    }

    let resolve = |target: u64| -> Result<Successor, ObfuscateError> {
        match resolve_jump_target(&instrs, end_bit, target)? {
            Some(instr_idx) => Ok(Some(block_of_instr[instr_idx])),
            None => Ok(None),
        }
    };

    // --- Determine each block's terminator, stripping it from the body. --
    let mut blocks = Vec::with_capacity(leaders.len());
    for (i, mut body) in block_bodies.into_iter().enumerate() {
        let fallthrough_bit = leaders.get(i + 1).copied().unwrap_or(end_bit);
        let terminator = match body.last().map(|instr| instr.opcode) {
            Some(OpCode::Halt) => {
                body.pop();
                Terminator::Halt
            }
            Some(OpCode::Jump) => {
                let Some(Operand::JumpTarget(target)) = body.pop().map(|i| i.operand) else {
                    unreachable!("just matched Some(OpCode::Jump)");
                };
                Terminator::Goto(resolve(target)?)
            }
            Some(OpCode::JumpIfFalse) => {
                let Some(Operand::JumpTarget(target)) = body.pop().map(|i| i.operand) else {
                    unreachable!("just matched Some(OpCode::JumpIfFalse)");
                };
                Terminator::Branch {
                    on_false: resolve(target)?,
                    on_true: resolve(fallthrough_bit)?,
                }
            }
            _ => Terminator::Goto(resolve(fallthrough_bit)?),
        };
        blocks.push(Block { body, terminator });
    }

    // --- Assign dispatch ids (a permutation of the blocks' new identity)
    // and a separate ladder-check order (a permutation of which original
    // block each position in the dispatcher's equality ladder tests) from
    // one continuing seeded stream. ---------------------------------------
    let mut rng = SplitMix64::new(seed);
    let ids = shuffled_indices(blocks.len(), &mut rng);
    let dispatch_order = shuffled_indices(blocks.len(), &mut rng);

    // --- Reassemble. -------------------------------------------------------
    let mut b = ProgramBuilder::new(new_local_count);
    for value in program.constants() {
        b.add_const(*value)?;
    }
    let id_const: Vec<u16> = ids
        .iter()
        .map(|&id| b.add_const(gindema_vm::Value::Int(id as i64)))
        .collect::<Result<_, _>>()?;

    let state_slot = program.local_count();
    let block_labels: Vec<LabelId> = (0..blocks.len()).map(|_| b.new_label()).collect();
    let dispatch_label = b.new_label();

    // Entry: jump straight into the dispatcher with the real entry block's
    // (block 0's, since leaders[0] is always bit 0) id already set.
    b.load_const(id_const[0]).store_local(state_slot);
    b.jump(dispatch_label);

    // The dispatcher: an equality ladder in `dispatch_order`, with no
    // check before the last case (every value the state slot can ever
    // hold is one of these ids, by construction, so the last remaining
    // possibility needs no test) -- see this function's own doc comment
    // and `docs/gindema-obfuscate.md`'s "Known limits" for what that
    // asymmetry gives away.
    b.place_label(dispatch_label);
    for (pos, &block_idx) in dispatch_order.iter().enumerate() {
        if pos + 1 < dispatch_order.len() {
            b.load_local(state_slot)
                .load_const(id_const[block_idx])
                .eq();
            let next_check = b.new_label();
            b.jump_if_false(next_check);
            b.jump(block_labels[block_idx]);
            b.place_label(next_check);
        } else {
            b.jump(block_labels[block_idx]);
        }
    }

    for (i, block) in blocks.iter().enumerate() {
        b.place_label(block_labels[i]);
        for instr in &block.body {
            emit_body_instr(&mut b, instr);
        }
        match block.terminator {
            Terminator::Halt => {
                b.halt();
            }
            Terminator::Goto(successor) => {
                emit_set_state_and_goto(&mut b, successor, &id_const, state_slot, dispatch_label);
            }
            Terminator::Branch { on_false, on_true } => {
                let false_label = b.new_label();
                b.jump_if_false(false_label);
                emit_set_state_and_goto(&mut b, on_true, &id_const, state_slot, dispatch_label);
                b.place_label(false_label);
                emit_set_state_and_goto(&mut b, on_false, &id_const, state_slot, dispatch_label);
            }
        }
    }

    Ok(b.build()?)
}

/// Re-emits one body instruction verbatim. `Halt`/`Jump`/`JumpIfFalse`
/// never reach here — they're always a block's [`Terminator`], stripped
/// from `body` before this is called.
fn emit_body_instr(b: &mut ProgramBuilder, instr: &DecodedInstr) {
    match (instr.opcode, instr.operand) {
        (OpCode::Pop, _) => {
            b.pop();
        }
        (OpCode::Dup, _) => {
            b.dup();
        }
        (OpCode::Swap, _) => {
            b.swap();
        }
        (OpCode::Add, _) => {
            b.add();
        }
        (OpCode::Sub, _) => {
            b.sub();
        }
        (OpCode::Mul, _) => {
            b.mul();
        }
        (OpCode::Div, _) => {
            b.div();
        }
        (OpCode::Neg, _) => {
            b.neg();
        }
        (OpCode::Not, _) => {
            b.not();
        }
        (OpCode::Eq, _) => {
            b.eq();
        }
        (OpCode::Lt, _) => {
            b.lt();
        }
        (OpCode::LoadConst, Operand::ConstIndex(idx)) => {
            b.load_const(idx);
        }
        (OpCode::LoadLocal, Operand::LocalSlot(slot)) => {
            b.load_local(slot);
        }
        (OpCode::StoreLocal, Operand::LocalSlot(slot)) => {
            b.store_local(slot);
        }
        _ => unreachable!(
            "Halt/Jump/JumpIfFalse are always stripped as a block's terminator before body \
             re-emission, and decode_program always pairs every other opcode with its one \
             matching Operand variant"
        ),
    }
}

/// Emits the tail every non-`Halt` terminator ends in: either an
/// unconditional hand-off to the dispatcher with `successor`'s id freshly
/// stored in the state slot, or `halt` for the implicit-end case.
fn emit_set_state_and_goto(
    b: &mut ProgramBuilder,
    successor: Successor,
    id_const: &[u16],
    state_slot: u8,
    dispatch_label: LabelId,
) {
    match successor {
        None => {
            b.halt();
        }
        Some(block_idx) => {
            b.load_const(id_const[block_idx])
                .store_local(state_slot)
                .jump(dispatch_label);
        }
    }
}

/// A full Fisher-Yates shuffle of `0..n`, continuing `rng`'s existing
/// stream rather than reseeding — so calling this twice in a row (once for
/// dispatch ids, once for the ladder's check order) gives two independent-
/// looking permutations from one seed, not the same permutation twice.
/// Distinct from `gindema-obfuscate`'s other shuffle
/// (`opcode_remap.rs::random_raw_values`, a *partial* shuffle that only
/// picks `OPCODE_COUNT` of 64 slots) — this one always permutes every
/// element, which that one's loop shape doesn't.
fn shuffled_indices(n: usize, rng: &mut SplitMix64) -> Vec<usize> {
    let mut perm: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.next_below(i + 1);
        perm.swap(i, j);
    }
    perm
}

#[cfg(test)]
mod tests {
    use super::*;
    use gindema_vm::{Value, Vm};

    fn straight_line_program() -> Program {
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
        b.build().unwrap()
    }

    fn if_else_program() -> Program {
        let mut b = ProgramBuilder::new(0);
        let one = b.add_const(Value::Int(1)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();
        let three = b.add_const(Value::Int(3)).unwrap();
        let four = b.add_const(Value::Int(4)).unwrap();
        let else_branch = b.new_label();
        let end = b.new_label();
        b.load_const(three).load_const(four).eq();
        b.jump_if_false(else_branch);
        b.load_const(one);
        b.jump(end);
        b.place_label(else_branch);
        b.load_const(two);
        b.place_label(end);
        b.halt();
        b.build().unwrap()
    }

    fn loop_program() -> Program {
        let mut b = ProgramBuilder::new(1);
        let zero = b.add_const(Value::Int(0)).unwrap();
        let one = b.add_const(Value::Int(1)).unwrap();
        let eleven = b.add_const(Value::Int(11)).unwrap();
        b.load_const(zero).store_local(0);
        let top = b.new_label();
        let end = b.new_label();
        b.place_label(top);
        b.load_local(0).load_const(eleven).lt();
        b.jump_if_false(end);
        b.load_local(0).load_const(one).add().store_local(0);
        b.jump(top);
        b.place_label(end);
        b.load_local(0);
        b.halt();
        b.build().unwrap()
    }

    #[test]
    fn flattening_straight_line_code_preserves_its_answer() {
        let original = straight_line_program();
        let flattened = flatten_control_flow(&original, 1).unwrap();
        assert_ne!(flattened.code_bytes(), original.code_bytes());

        let mut vm = Vm::new();
        assert_eq!(vm.run(&flattened).unwrap(), Some(Value::Int(20)));
    }

    fn if_true_branch_program() -> Program {
        // The condition is a literal `true == true`, so `JumpIfFalse` never
        // takes the jump -- this directly exercises the `Branch`
        // terminator's `on_true` arm without needing a loop to eventually
        // reach it, unlike `if_else_program` below.
        let mut b = ProgramBuilder::new(0);
        let one = b.add_const(Value::Int(1)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();
        let t1 = b.add_const(Value::Bool(true)).unwrap();
        let t2 = b.add_const(Value::Bool(true)).unwrap();
        let else_branch = b.new_label();
        let end = b.new_label();
        b.load_const(t1).load_const(t2).eq();
        b.jump_if_false(else_branch);
        b.load_const(one);
        b.jump(end);
        b.place_label(else_branch);
        b.load_const(two);
        b.place_label(end);
        b.halt();
        b.build().unwrap()
    }

    #[test]
    fn flattening_an_if_taking_the_true_branch_directly_exercises_on_true() {
        let original = if_true_branch_program();
        let flattened = flatten_control_flow(&original, 8).unwrap();
        let mut vm = Vm::new();
        assert_eq!(vm.run(&flattened).unwrap(), Some(Value::Int(1)));
    }

    #[test]
    fn flattening_an_if_else_preserves_both_outcomes() {
        // The condition here is a compile-time-constant `false`, so this
        // only directly exercises the `else` arm -- the loop test below
        // (whose condition genuinely depends on runtime state) is what
        // actually proves the `JumpIfFalse` translation's `on_true` arm
        // works, since its loop body only continues while the real,
        // changing counter is still true.
        let original = if_else_program();
        let flattened = flatten_control_flow(&original, 2).unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&flattened).unwrap(), Some(Value::Int(2)));
    }

    #[test]
    fn flattening_a_loop_preserves_both_the_backward_jump_and_the_exit() {
        let original = loop_program();
        let flattened = flatten_control_flow(&original, 3).unwrap();

        let mut vm_a = Vm::new();
        let mut vm_b = Vm::new();
        assert_eq!(vm_a.run(&original).unwrap(), vm_b.run(&flattened).unwrap());
        assert_eq!(vm_b.run(&flattened).unwrap(), Some(Value::Int(11)));
    }

    #[test]
    fn flattening_a_program_with_a_honeypot_keeps_the_honeypot_unreachable() {
        let mut b = ProgramBuilder::new(0);
        let one = b.add_const(Value::Int(1)).unwrap();
        let two = b.add_const(Value::Int(2)).unwrap();
        b.load_const(one);
        crate::inject_honeypot(&mut b, 42).unwrap();
        b.load_const(two).add().halt();
        let original = b.build().unwrap();

        let flattened = flatten_control_flow(&original, 4).unwrap();
        let mut vm = Vm::new();
        // Same reasoning as honeypot.rs's own test: the dead end halts
        // with fake arithmetic, so getting the real answer back proves it
        // stayed unreachable through flattening too, not just that the
        // stack happened to look right.
        assert_eq!(vm.run(&flattened).unwrap(), Some(Value::Int(3)));
    }

    #[test]
    fn flattening_twice_still_produces_a_correct_program() {
        let original = loop_program();
        let once = flatten_control_flow(&original, 5).unwrap();
        let twice = flatten_control_flow(&once, 6).unwrap();

        let mut vm = Vm::new();
        assert_eq!(vm.run(&twice).unwrap(), Some(Value::Int(11)));
    }

    #[test]
    fn same_seed_is_deterministic_different_seeds_usually_differ() {
        let original = loop_program();
        let a = flatten_control_flow(&original, 10).unwrap();
        let b = flatten_control_flow(&original, 10).unwrap();
        assert_eq!(a.code_bytes(), b.code_bytes());

        let c = flatten_control_flow(&original, 11).unwrap();
        assert_ne!(a.code_bytes(), c.code_bytes());
    }

    #[test]
    fn flattening_reserves_exactly_one_more_local_than_the_original() {
        let original = loop_program();
        let flattened = flatten_control_flow(&original, 7).unwrap();
        assert_eq!(flattened.local_count(), original.local_count() + 1);
    }

    #[test]
    fn an_empty_program_flattens_to_an_equally_empty_one() {
        let original = ProgramBuilder::new(2).build().unwrap();
        let flattened = flatten_control_flow(&original, 1).unwrap();
        assert_eq!(flattened.local_count(), 2);
        assert_eq!(flattened.code_bit_len(), 0);
    }

    #[test]
    fn a_program_already_at_the_local_limit_is_rejected_not_overflowed() {
        let original = ProgramBuilder::new(u8::MAX).halt().build().unwrap();
        assert_eq!(
            flatten_control_flow(&original, 1),
            Err(ObfuscateError::TooManyLocals)
        );
    }
}
