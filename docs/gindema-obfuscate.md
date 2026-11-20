# gindema-obfuscate

## Overview

Compiler passes that run over `gindema-vm` bytecode: control-flow
flattening, a per-build opcode-table remap, state-dependent (fluff-byte)
decoding, and honeypot/dead-end function injection. Two of the four are
implemented now:

- **Per-build opcode remap**, over an already-built `Program`: tested, and
  wired into the real mechanism `gindema-vm` added for it
  (`OpcodeMap`/`remap_program`, see `docs/gindema-vm.md`).
- **Honeypot/dead-end injection**, while a `Program` is still being
  *built*: `inject_honeypot` emits a conditional branch that always takes
  the same path, guarding a block of plausible fake computation that ends
  in its own `Halt` and is never reached. Unlike opcode remap, this needed
  no changes to `gindema-vm` at all — it's built entirely on
  `ProgramBuilder`'s existing label/jump API.

Control-flow flattening and state-dependent decoding have no
implementation here at all yet; see "Known limits".

**Mechanism lives in `gindema-vm`, policy lives here — except where there's
no mechanism to separate out.** For opcode remap, `gindema-vm` knows how to
losslessly re-encode its own bytecode stream under a different numbering —
that's `remap_program` — and has no opinion on where the new numbering
comes from; this crate supplies that (a seeded, pseudorandom permutation).
Honeypot injection doesn't split the same way: there's no new bytecode
*format* feature to add to `gindema-vm` for it, since a honeypot is just
more ordinary instructions placed at an ordinary point in an ordinary
build — so the entire pass lives here, using tools `gindema-vm` already
exposed for other reasons. Either way, `gindema-vm`'s own tests stay fully
deterministic (no RNG in that crate at all); this crate owns everything
about *how* a build's randomization actually happens.

## Modules

### `lib.rs`
**What it does:** crate doc (with two runnable examples — one builds a
tiny program, remaps it, runs it, same answer; the other builds a program
with a honeypot inlined, runs it, confirms the honeypot was skipped),
module wiring, flat re-exports (`remap_opcodes`, `random_opcode_map`,
`ObfuscateError`, `inject_honeypot`).

### `rng.rs`
**What it does:** `SplitMix64` — the same generator choice `mirage-train`
made for its own seeded shuffling (see `docs/mirage-train.md`). `next_u64`
is the textbook SplitMix64 step; `next_below(bound)` is plain modulo, not
perfectly uniform for a non-power-of-two `bound` — an accepted bias here,
since this is build-to-build *variation*, not a cryptographic shuffle
trying to resist a statistical attack. Not exported from the crate (`pub(crate)`
only) — nothing outside this crate's own obfuscation logic needs a raw RNG.

**Tests:** inline. Same seed reproduces the same sequence exactly; different
seeds diverge; `next_below` never returns an out-of-range value across 200
draws.

### `opcode_remap.rs`
**What it does:** `random_raw_values` — a partial Fisher-Yates shuffle of a
64-slot pool, taking the first `OPCODE_COUNT` (17) slots as the new raw
value for each of `OpCode::ALL`'s opcodes, in order. By construction this
always produces `OPCODE_COUNT` pairwise-distinct values each `< 64` — so
`random_opcode_map`'s call to `OpcodeMap::from_permutation` is backed by an
`.expect(...)` with the invariant spelled out, not a propagated error:
there is no permutation this function can hand it that
`from_permutation` would reject.

`remap_opcodes(program, seed)` is the actual entry point: builds a random
map from `seed`, calls `gindema_vm::remap_program`, and returns the result
(or `ObfuscateError::Vm` if the underlying call failed — only possible for
a `Program` that didn't come from `ProgramBuilder`, since one that did is
always well-formed by construction).

**On returning just the `Program`, not the `Program` plus its map
separately:** the returned `Program` already carries the `OpcodeMap` it was
encoded with (see `docs/gindema-vm.md`'s `program.rs` section) and
`Vm::run` reads that automatically, so there is nothing a caller needs to
track alongside the program for it to execute correctly. Anyone that does
want to inspect which map a given remap actually produced can still call
`Program::opcode_map()` on the result.

**Tests:** inline. `random_raw_values` checked across 50 seeds for
distinctness and range. The same seed gives the same map twice; seed 123
vs. 456 gives different maps (not proven for *all* seed pairs — two
different seeds could in principle land on the same permutation, just
vanishingly unlikely among `17!`-ish outcomes — this test picks two fixed
seeds and checks the actual, expected-to-differ case rather than trying to
prove the general property). `remap_opcodes` end-to-end on a real program
with a loop (so jump targets are exercised): the remapped bytes differ,
the map isn't identity, and running the original versus the remapped
program through separate `Vm`s gives the identical result. Remapping the
same program with two different seeds gives two different byte encodings
that still both run to the same answer.

### `honeypot.rs`
**What it does:** `inject_honeypot(&mut ProgramBuilder, seed)` — emits, at
the builder's current position, two constant-pool entries holding the same
value (so the branch reads as a comparison between two named values, not
an obviously-constant-folded no-op), `Eq`/`Not`/`JumpIfFalse` (always
true → always false → always jumps, so the branch is unconditionally
taken, every run, regardless of program state), a short run of fake
arithmetic over random-looking constants, and a `Halt` — then the skip
label. The fake block is a dead end in the literal sense: were it ever
reached, the program would stop right there.

No changes to `gindema-vm` were needed: everything this uses
(`new_label`/`place_label`/`jump_if_false`/`add_const`/`eq`/`not`/
arithmetic/`halt`) already existed on `ProgramBuilder` for ordinary
program construction. Calling this while building a real program — at
whatever point the caller chooses, as many times as the caller chooses —
is the entire integration surface; there's no separate "honeypot API"
beyond the one function.

**Tests:** inline. The core property: a program with a honeypot inlined
between two real instructions still produces the real answer — and
specifically because the dead end ends in its own `Halt`, getting the real
final answer is a strong signal the honeypot was actually skipped (a
version of this test with stack-neutral-but-non-halting dead code
wouldn't prove nearly as much, since a stack-neutral block would leave the
right answer behind whether or not it actually ran). Also: a honeypot
genuinely adds bytes (not a no-op); the same seed produces byte-identical
output, different seeds usually don't; three honeypots stitched into one
program all stay unreachable together.

## Known limits (intentional, for this pass)

- **Control-flow flattening:** not implemented. No code here changes a
  program's control-flow shape at all.
- **State-dependent (fluff-byte) decoding:** not implemented. The bytecode
  stream `remap_opcodes` produces is just as directly decodable as the
  input, modulo the opcode table itself being scrambled — there's no
  decode-time state machine or junk-byte interleaving yet.
- **Remap strength:** `SplitMix64` plus modulo-biased sampling is explicitly
  not cryptographic — see `rng.rs`'s note. The opcode space is also small
  (17 opcodes into 64 slots): a determined attacker with the published
  `gindema-cli` binary in hand and some patience could plausibly recover
  the mapping by correlating decoded behavior against known-good test
  inputs, especially with no decoding-side obfuscation (the bullet above)
  layered on top yet. This pass raises the bar against casual static
  analysis; it is not presented as resistant to a dedicated reverse-engineer.
- **Honeypot strength:** the always-taken-branch pattern (`Eq` of two
  equal-valued constants, `Not`, `JumpIfFalse`) is itself a fixed,
  recognizable shape — the same four-opcode sequence every time, just with
  different constant values. Anyone who reads this crate's source (it's
  vendored into a proprietary repo, not published, but that's not the same
  as unrecoverable) learns the exact pattern to grep decoded bytecode for.
  Real strength here would need the pattern itself to vary per use, or to
  be layered under opcode remap and the not-yet-built state-dependent
  decoding so the sequence isn't sitting in the clear to begin with. As
  implemented, this raises the bar for a quick skim of disassembled
  output; it does not hide from someone who read this source first.
- **No build-pipeline integration yet, for either implemented pass:**
  nothing here is wired into `gindema-cli` or any build/packaging step.
  `remap_opcodes` and `inject_honeypot` are both library functions a
  future build step (or a future compiler emitting `gindema-vm` bytecode
  in the first place) would call — `remap_opcodes` once per build with a
  fresh seed, `inject_honeypot` at whatever points in a real program the
  caller chooses — not something that runs automatically today.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-obfuscate` runs 11 unit tests + 2 doctests; `cargo
clippy -p gindema-obfuscate --all-targets -- -D warnings` and `cargo fmt -p
gindema-obfuscate -- --check` are both clean. Verified in the sandbox on
Rust 1.100.

## Fixes and Problems

- None from either pass so far — both designs (mechanism in `gindema-vm`
  with randomness policy here for opcode remap; entirely within this crate,
  reusing `ProgramBuilder`'s existing API, for honeypot injection) worked
  on the first implementation attempt. Opcode remap is verified by the
  "remap and run both ways" tests in `opcode_remap.rs`; honeypot injection
  by the dead-end-ends-in-Halt tests in `honeypot.rs` (see that module's
  own note on why that specific test design is the strong version of the
  claim, not just a weaker stack-balance check).
