# gindema-obfuscate

## Overview

Compiler passes that run over `gindema-vm` bytecode: control-flow
flattening, a per-build opcode-table remap, state-dependent (fluff-byte)
decoding, and honeypot/dead-end function injection. All four are now
implemented:

- **Per-build opcode remap**, over an already-built `Program`: tested, and
  wired into the real mechanism `gindema-vm` added for it
  (`OpcodeMap`/`remap_program`, see `docs/gindema-vm.md`).
- **Honeypot/dead-end injection**, while a `Program` is still being
  *built*: `inject_honeypot` emits a conditional branch that always takes
  the same path, guarding a block of plausible fake computation that ends
  in its own `Halt` and is never reached. Unlike opcode remap, this needed
  no changes to `gindema-vm` at all — it's built entirely on
  `ProgramBuilder`'s existing label/jump API.
- **Control-flow flattening**, over an already-built `Program`:
  `flatten_control_flow` decodes it into basic blocks (via
  `gindema_vm::decode_program`) and rebuilds it as a single dispatcher
  loop, so every inter-block transfer — including an ordinary block's own
  fallthrough into the next one — reads as "set a state local, jump back
  to the dispatcher" instead of a direct jump to wherever the destination
  happens to sit.
- **State-dependent decoding**, over an already-built `Program`:
  `obfuscate_with_fluff` is a thin wrapper over
  `gindema_vm::interleave_fluff` — the real mechanism, and the one place
  this needed a genuine `gindema-vm` format change rather than living
  entirely in this crate.

**Mechanism lives in `gindema-vm`, policy lives here — except where there's
no mechanism to separate out, or where the mechanism needed is almost
entirely policy-free.** Opcode remap: `gindema-vm` knows how to losslessly
re-encode its own bytecode stream under a different numbering
(`remap_program`) and has no opinion on where the numbering comes from;
this crate supplies a seeded permutation. Honeypot injection doesn't split
the same way: there's no new bytecode *format* feature needed for it, since
a honeypot is just more ordinary instructions at an ordinary point in an
ordinary build — the entire pass lives here. Control-flow flattening's
*mechanism* is `gindema-vm`'s `decode_program` (reading a built program back
into a structured instruction list) — genuinely reusable infrastructure,
not flattening-specific — but the actual block-finding and
dispatcher-building logic is entirely this crate's, same as any other
compiler pass would be. State-dependent decoding inverts the usual split:
the *mechanism* (`interleave_fluff`) needed real `gindema-vm`-level work,
since interleaving junk bits moves every instruction after the first fluff
region and something has to recompute every jump target that's now
invalid — but the *policy* here is almost a no-op (there's no permutation
to choose, just a seed to pass through), unlike opcode remap's real
permutation-generation job. Throughout, `gindema-vm`'s own tests stay fully
deterministic (no RNG in that crate at all, even for `interleave_fluff` —
it takes a seed as a plain argument); this crate owns everything about
*how* a build's randomization actually happens.

**Composability across all four passes** is deliberate, not incidental —
see `docs/gindema-vm.md`'s `fluff.rs` section for the mechanism-level half
of this, and this doc's own `flatten.rs`/`fluff.rs` sections below for the
policy-level half. In short: honeypot injection happens during the
original `ProgramBuilder` construction, before any of the other three ever
run. After that, flatten, fluff, and opcode remap can each take any
already-built `Program` (including one the others have already
transformed) — except flattening always rebuilds via `ProgramBuilder` from
scratch, so it silently discards any existing fluff or opcode remap in its
input. The one correct pipeline order is therefore **honeypot (at build
time) → flatten → fluff → opcode remap**; this crate's own `lib.rs` test
module exercises exactly that chain end to end, plus a deliberate
out-of-order case (fluff before flatten) to confirm it still produces a
*correct* program even though it discards the fluff, rather than something
subtly wrong.

## Modules

### `lib.rs`
**What it does:** crate doc (four runnable examples now — opcode remap,
honeypot injection, control-flow flattening on a small if/else, and
state-dependent decoding on a one-instruction program), module wiring,
flat re-exports (`remap_opcodes`, `random_opcode_map`, `ObfuscateError`,
`inject_honeypot`, `flatten_control_flow`, `obfuscate_with_fluff`). Also
owns this crate's one `#[cfg(test)] mod tests` block, specifically for
composability: every other module's tests are thorough about the one pass
they add, but nothing in any single module's tests chains all four passes
together the way a real build pipeline eventually will — see the Overview
section above for the pipeline order those tests check.

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

### `flatten.rs`
**What it does:** `flatten_control_flow(&Program, seed: u64) ->
Result<Program, ObfuscateError>`. Decodes `program` via
`gindema_vm::decode_program`, finds basic-block leaders the standard way
(bit 0; every `Jump`/`JumpIfFalse` target; the bit offset right after
every `Jump`/`JumpIfFalse`, covering both a conditional's real fallthrough
and a `Jump`'s own dead code afterward — a honeypot's dead end needs
exactly this same treatment, see `honeypot.rs` above), partitions
instructions into blocks on those boundaries, and determines each block's
`Terminator` (`Halt`; `Goto` for a block ending in `Jump` *or* one that
just falls into the next block with no jump at all — both are "always go
here next"; `Branch` for `JumpIfFalse`, with `on_false`/`on_true`
resolved via `gindema_vm::resolve_jump_target` the same way `Goto`'s
target is). Reassembles the whole thing as one dispatcher loop: a new
local slot holds the current block's shuffled id; the dispatcher is a flat
equality ladder (in a second, independent shuffled order — which block id
is checked at which ladder position) with no comparison before the very
last case, since by construction the state slot always holds one of the
valid ids, so the last remaining possibility needs no test; every
`Terminator` becomes "set the successor block's id, jump back to the
dispatcher" (or `halt`, for the implicit-end case
`resolve_jump_target`/`Program::code_bit_len` already define).

**The one place a direct branch still remains:** `JumpIfFalse` is this
instruction set's only conditional primitive, and there's no select/
ternary opcode to pick between two "which id to set" constants without
one. So a `Branch` terminator still compiles to one real `jump_if_false`
— just one that decides between two short, straight-line "set id, jump to
dispatcher" snippets instead of between two arbitrary, structurally
meaningful destinations. See "Known limits" below for exactly how much of
the original shape that still reveals.

**Two independent shuffles from one continuing `SplitMix64` stream, not
one reused for both:** `ids` (which numeric value represents each
original block) and `dispatch_order` (which order the dispatcher's ladder
tests those ids in) are deliberately separate permutations — reusing one
for both would make the ladder's check order trivially predictable from
the ids themselves (or vice versa). `shuffled_indices`, this file's own
full Fisher-Yates shuffle, is distinct from `opcode_remap.rs`'s
`random_raw_values`, which only *partially* shuffles (picks `OPCODE_COUNT`
of 64 slots) — flattening always needs every block permuted, not a subset
of a larger pool.

**Reserves exactly one more local slot than the input program declared**
(`program.local_count() + 1`, for the dispatcher's own state variable) —
`ObfuscateError::TooManyLocals` if the input already uses all 255 a `u8`
count allows, rather than silently wrapping.

**An empty (zero-instruction) program is a documented degenerate case,**
not an error: `ProgramBuilder::new(n).build()` with nothing ever emitted
produces exactly this, and flattening it just returns an equally empty
program with the same local count — there's no control flow to flatten.

**Tests:** inline, 12. Straight-line code, an if/else actually taking the
constant-`false` path, a second if/else actually taking the constant-`true`
path (so the `Branch` terminator's `on_true` arm is exercised directly,
not only incidentally via the loop test), and a real counting loop
(backward jump) all preserve their original answer; flattening a program
with a honeypot inlined keeps the honeypot unreachable (same dead-end-
ends-in-`Halt` reasoning as `honeypot.rs`'s own tests); flattening twice in
a row still produces a correct program; the same seed is byte-for-byte
deterministic, different seeds usually differ; flattening reserves exactly
one more local than the input; an already-maximal local count is rejected
without overflowing; an empty program flattens to an equally empty one.

### `fluff.rs`
**What it does:** `obfuscate_with_fluff(&Program, seed: u64) ->
Result<Program, ObfuscateError>` — calls straight through to
`gindema_vm::interleave_fluff` and wraps its error. See
`docs/gindema-vm.md`'s `fluff.rs` section for the actual mechanism (why
bit *position*, not instruction *index*, is the key that makes jumping
into fluffed bytecode work, and how this composes with the other three
passes). Exists mainly so every obfuscation pass is reached the same way,
through this crate, rather than three living here and the fourth needing
a direct `gindema_vm` import — there's no permutation-generation job
comparable to `random_opcode_map`'s for this one to do.

**Tests:** inline, 2 (deliberately light — the real behavior is
`gindema-vm`'s to test, thoroughly, in its own `fluff.rs`; this file's
tests only confirm the wrapper itself is wired correctly). Wraps
`interleave_fluff` and the result still runs correctly; the same seed is
deterministic.

## Known limits (intentional, for this pass)

- **Flattening strength:** the dispatcher's last ladder case has no
  comparison before it (see `flatten.rs`'s own notes above) — a structural
  tell that distinguishes it from every other case, for anyone studying
  the flattened bytecode directly rather than just running it. The
  residual `JumpIfFalse` a `Branch` terminator still compiles to is itself
  a recognizable, fixed shape (two short "set id, jump to dispatcher"
  snippets either side of one conditional jump) once someone's read this
  source — real flattening implementations sometimes also scramble block
  *body* layout independently of the dispatcher's check order, or flatten
  the dispatcher's own ladder into something less uniformly shaped; this
  pass does neither. As implemented, this defeats a naive "follow the
  jumps" CFG recovery; it does not hide the dispatcher pattern itself from
  someone who's read this crate's source.
- **Fluff strength:** `MAX_FLUFF_BITS = 7` keeps average overhead to about
  3.5 bits per instruction — modest by design (a cheap layer under the
  other three passes, not a payload in its own right), which also means
  it's a small search space: someone who suspects fluff and has the real
  seed (or guesses a small one) can verify it trivially by replaying
  `fluff_len_at`. `SplitMix64` is explicitly non-cryptographic (see
  `gindema_common::SplitMix64`'s own note) — nothing here assumes the
  fluff lengths themselves resist a determined statistical attack if the
  underlying generator's weaknesses are known. The real strength this pass
  offers is structural, not cryptographic: a disassembler that doesn't
  track "which bit position am I logically at" — i.e. doesn't simulate the
  same fetch-decode walk `Vm::run` does — has no way to know where real
  instructions even start, since there's no length prefix or marker to
  find them by.
- **Remap strength:** `SplitMix64` plus modulo-biased sampling is explicitly
  not cryptographic — see `gindema_common::SplitMix64`'s own note. The
  opcode space is also small
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
- **No build-pipeline integration yet, for any of the four passes:**
  nothing here is wired into `gindema-cli` or any build/packaging step
  (unlike `gindema-vm`'s own `assemble`, which `gindema-cli` does expose as
  a subcommand). `remap_opcodes`, `inject_honeypot`,
  `flatten_control_flow`, and `obfuscate_with_fluff` are all library
  functions a future build step (or a future compiler emitting
  `gindema-vm` bytecode in the first place) would call in the pipeline
  order this doc's Overview section describes — not something that runs
  automatically today.
- **Flattening always produces a clean, un-fluffed, identity-opcode-map
  result,** since it rebuilds entirely from scratch via `ProgramBuilder`.
  This is why the Overview section above states flattening has to come
  *before* fluff and opcode remap in a pipeline, not after or between them
  — applying either of those first and flattening afterward silently
  discards them, which `lib.rs`'s own composability tests confirm is at
  least *correct* (the program still runs right) even though it isn't
  what a caller doing that probably intended.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-obfuscate` runs 22 unit tests + 4 doctests (up from
11 + 2: 12 new in `flatten.rs`, 2 new in `fluff.rs`, 2 new in `lib.rs`'s
composability test module, 2 new doctests for the two new passes); `cargo
clippy -p gindema-obfuscate --all-targets -- -D warnings` and `cargo fmt -p
gindema-obfuscate -- --check` are both clean. Verified in the sandbox on
Rust 1.100 — not yet confirmed on the real GitHub Actions runner (that
confirmation at commit `f8e0115` predates this session's flattening/fluff
work; see the root `README.md`/`CHANGELOG.md`).

## Fixes and Problems

- **The real design problem this session, solved in `gindema-vm` rather
  than here:** state-dependent decoding's first design that comes to
  mind — a fluff length per sequential instruction *index*, drawn from one
  continuously-advancing RNG stream — breaks the moment a `Jump` lands
  somewhere other than "the next instruction in program order," since the
  decoder would need to know which index it just landed on to resume the
  stream correctly. See `docs/gindema-vm.md`'s `fluff.rs` section for the
  fix (keying fluff length on the reader's current *bit position* instead,
  which needs no index, counter, or table at all) and its own "Fixes and
  Problems" entry recording this as a design decision, not a bug caught
  after the fact — it was worked out before any code was written, not
  discovered by a failing test.
- `rng.rs` (`SplitMix64`) was deleted from this crate and moved, unchanged,
  into `gindema-common` once `gindema-container` needed the identical
  generator — `opcode_remap.rs`/`honeypot.rs` now call
  `gindema_common::SplitMix64` instead. Same one-manual-step caveat as
  `gindema-guard`'s own `checksum.rs` move: the delivery zip can only add
  or overwrite files, so `crates/gindema-obfuscate/src/rng.rs` will still
  physically exist in the live repo after this delta, unreferenced
  (`lib.rs` no longer has `mod rng;`) but harmless. Delete it by hand
  alongside `gindema-guard`'s leftover `checksum.rs` when convenient.
- None from the first two passes — both designs (mechanism in `gindema-vm`
  with randomness policy here for opcode remap; entirely within this crate,
  reusing `ProgramBuilder`'s existing API, for honeypot injection) worked
  on the first implementation attempt. Opcode remap is verified by the
  "remap and run both ways" tests in `opcode_remap.rs`; honeypot injection
  by the dead-end-ends-in-Halt tests in `honeypot.rs` (see that module's
  own note on why that specific test design is the strong version of the
  claim, not just a weaker stack-balance check).
