// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-obfuscate.md, section "opcode_remap.rs"
// ============================================================================

use std::fmt;

use gindema_vm::{remap_program, BuildError, OpcodeMap, Program, VmError, OPCODE_COUNT};

use gindema_common::SplitMix64;

/// Everything that can go wrong in one of this crate's passes.
///
/// `Vm` (from opcode remap, flattening, or the fluff wrapper's underlying
/// [`gindema_vm::decode_program`]/[`gindema_vm::interleave_fluff`] calls)
/// and `Build` (from flattening's own [`gindema_vm::ProgramBuilder`] calls
/// while reassembling a flattened program) can, in practice, only happen
/// for a [`Program`] that didn't come from `ProgramBuilder` in the first
/// place — one built the normal way is always well-formed, so running any
/// of these passes on it can't fail that way. The errors are still
/// surfaced rather than assumed away, since this crate doesn't control how
/// every `Program` it's ever handed got built. `TooManyLocals` is
/// flattening's own, separate concern — see [`crate::flatten_control_flow`].
#[derive(Debug, Clone, PartialEq)]
pub enum ObfuscateError {
    /// An underlying `gindema-vm` call failed — the program's own bytecode
    /// stream was malformed.
    Vm(VmError),
    /// Reassembling a transformed program via [`gindema_vm::ProgramBuilder`]
    /// failed.
    Build(BuildError),
    /// [`crate::flatten_control_flow`] needs one more local slot than the
    /// input program declared, and the input already declared the maximum
    /// a `u8` count allows (255).
    TooManyLocals,
}

impl fmt::Display for ObfuscateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vm(e) => write!(f, "{e}"),
            Self::Build(e) => write!(f, "{e}"),
            Self::TooManyLocals => {
                write!(f, "program already uses the maximum 255 local slots")
            }
        }
    }
}

impl std::error::Error for ObfuscateError {}

impl From<VmError> for ObfuscateError {
    fn from(e: VmError) -> Self {
        Self::Vm(e)
    }
}

impl From<BuildError> for ObfuscateError {
    fn from(e: BuildError) -> Self {
        Self::Build(e)
    }
}

/// Generates a random permutation of `OPCODE_COUNT` distinct values drawn
/// from `0..64`, via a partial Fisher-Yates shuffle: for each of the first
/// `OPCODE_COUNT` positions, swap in a uniform-ish remaining value. The
/// result is always `OPCODE_COUNT` pairwise-distinct values each `< 64` —
/// exactly what [`OpcodeMap::from_permutation`] requires — by construction,
/// not by validation after the fact.
fn random_raw_values(rng: &mut SplitMix64) -> [u8; OPCODE_COUNT] {
    let mut pool: [u8; 64] = {
        let mut p = [0u8; 64];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        p
    };
    for i in 0..OPCODE_COUNT {
        let j = i + rng.next_below(64 - i);
        pool.swap(i, j);
    }
    let mut result = [0u8; OPCODE_COUNT];
    result.copy_from_slice(&pool[..OPCODE_COUNT]);
    result
}

/// Builds a random, non-identity [`OpcodeMap`] from `seed`. Same `seed` →
/// same map, always — this is a build-reproducibility property, not a
/// security one (see `gindema_common::SplitMix64`'s own notes on why).
pub fn random_opcode_map(seed: u64) -> OpcodeMap {
    let mut rng = SplitMix64::new(seed);
    let raw_values = random_raw_values(&mut rng);
    OpcodeMap::from_permutation(raw_values).expect(
        "random_raw_values always returns OPCODE_COUNT pairwise-distinct values each < 64, \
         which OpcodeMap::from_permutation always accepts",
    )
}

/// Per-build opcode remap: re-encodes `program` under a random opcode
/// numbering derived from `seed`, so two builds of the same source program
/// disagree about which raw byte means which instruction. See
/// `docs/gindema-obfuscate.md` and `docs/gindema-vm.md`'s `remap.rs`
/// section for why this changes nothing about the program's behavior,
/// length, or jump targets — only which numbers its opcodes are written as.
///
/// The returned [`Program`] carries its own [`OpcodeMap`] internally
/// (`gindema_vm::Vm::run` reads it automatically), so nothing further needs
/// to be tracked alongside it for the program to run correctly. Call
/// [`Program::opcode_map`](gindema_vm::Program::opcode_map) on the result if
/// something else needs to inspect which map was actually used.
///
/// # Errors
/// See [`ObfuscateError`].
pub fn remap_opcodes(program: &Program, seed: u64) -> Result<Program, ObfuscateError> {
    let map = random_opcode_map(seed);
    Ok(remap_program(program, &map)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gindema_vm::{ProgramBuilder, Value, Vm};

    fn sample_program() -> Program {
        let mut b = ProgramBuilder::new(1);
        let five = b.add_const(Value::Int(5)).unwrap();
        let one = b.add_const(Value::Int(1)).unwrap();
        let zero = b.add_const(Value::Int(0)).unwrap();
        b.load_const(zero).store_local(0);
        let top = b.new_label();
        let end = b.new_label();
        b.place_label(top);
        b.load_local(0).load_const(five).lt();
        b.jump_if_false(end);
        b.load_local(0).load_const(one).add().store_local(0);
        b.jump(top);
        b.place_label(end);
        b.load_local(0);
        b.halt();
        b.build().unwrap()
    }

    #[test]
    fn random_raw_values_are_always_distinct_and_in_range() {
        for seed in 0..50u64 {
            let mut rng = SplitMix64::new(seed);
            let values = random_raw_values(&mut rng);
            for &v in &values {
                assert!(v < 64);
            }
            for i in 0..values.len() {
                for j in (i + 1)..values.len() {
                    assert_ne!(values[i], values[j], "seed {seed} produced a duplicate");
                }
            }
        }
    }

    #[test]
    fn same_seed_gives_the_same_map_different_seeds_usually_differ() {
        let map_a = random_opcode_map(123);
        let map_b = random_opcode_map(123);
        assert_eq!(map_a, map_b);

        let map_c = random_opcode_map(456);
        assert_ne!(map_a, map_c);
    }

    #[test]
    fn remap_opcodes_changes_the_bytes_but_not_the_behavior() {
        let original = sample_program();
        let remapped = remap_opcodes(&original, 7).unwrap();

        assert_ne!(remapped.code_bytes(), original.code_bytes());
        assert_ne!(remapped.opcode_map(), &OpcodeMap::identity());

        let mut vm_a = Vm::new();
        let mut vm_b = Vm::new();
        assert_eq!(vm_a.run(&original).unwrap(), vm_b.run(&remapped).unwrap());
    }

    #[test]
    fn two_different_seeds_produce_two_different_encodings_of_the_same_program() {
        let original = sample_program();
        let remapped_a = remap_opcodes(&original, 1).unwrap();
        let remapped_b = remap_opcodes(&original, 2).unwrap();
        assert_ne!(remapped_a.code_bytes(), remapped_b.code_bytes());

        let mut vm_a = Vm::new();
        let mut vm_b = Vm::new();
        assert_eq!(
            vm_a.run(&remapped_a).unwrap(),
            vm_b.run(&remapped_b).unwrap()
        );
    }
}
