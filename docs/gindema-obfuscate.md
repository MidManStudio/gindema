# gindema-obfuscate

## Overview

Compiler passes that run over an already-built `gindema-vm` `Program`:
control-flow flattening, a per-build opcode-table remap, state-dependent
(fluff-byte) decoding, and honeypot/dead-end function injection. This pass
implements the first of those — **per-build opcode remap** — end to end,
tested, and wired into the real mechanism `gindema-vm` added for it
(`OpcodeMap`/`remap_program`, see `docs/gindema-vm.md`). The other three
have no implementation here at all yet; see "Known limits".

**Mechanism lives in `gindema-vm`, policy lives here.** `gindema-vm` knows
how to losslessly re-encode its own bytecode stream under a different
opcode numbering — that's `remap_program`, and it has no opinion on where
the new numbering comes from. This crate supplies that: a seeded,
pseudorandom permutation, generated fresh per call to
`remap_opcodes`. Splitting it this way means `gindema-vm`'s own tests stay
fully deterministic (no RNG in that crate at all) while this crate owns
everything about *how* a build's randomization actually happens.

## Modules

### `lib.rs`
**What it does:** crate doc (with a runnable example: build a tiny program,
remap it, run it, same answer), module wiring, flat re-exports
(`remap_opcodes`, `random_opcode_map`, `ObfuscateError`).

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

## Known limits (intentional, for this pass)

- **Control-flow flattening:** not implemented. No code here changes a
  program's control-flow shape at all.
- **State-dependent (fluff-byte) decoding:** not implemented. The bytecode
  stream `remap_opcodes` produces is just as directly decodable as the
  input, modulo the opcode table itself being scrambled — there's no
  decode-time state machine or junk-byte interleaving yet.
- **Honeypot/dead-end injection:** not implemented. Nothing here inserts
  fake branches, fake functions, or anything a static analysis would need
  to specifically detect and avoid.
- **Remap strength:** `SplitMix64` plus modulo-biased sampling is explicitly
  not cryptographic — see `rng.rs`'s note. The opcode space is also small
  (17 opcodes into 64 slots): a determined attacker with the published
  `gindema-cli` binary in hand and some patience could plausibly recover
  the mapping by correlating decoded behavior against known-good test
  inputs, especially with no decoding-side obfuscation (the next bullet)
  layered on top yet. This pass raises the bar against casual static
  analysis; it is not presented as resistant to a dedicated reverse-engineer.
- **No build-pipeline integration yet:** nothing here is wired into
  `gindema-cli` or any build/packaging step — `remap_opcodes` is a library
  function a future build step would call with a fresh per-build seed (a
  real random source, a build timestamp, whatever the pipeline decides),
  not something that runs automatically today.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only).
`cargo test -p gindema-obfuscate` runs 7 unit tests + 1 doctest; `cargo
clippy -p gindema-obfuscate --all-targets -- -D warnings` and `cargo fmt -p
gindema-obfuscate -- --check` are both clean. Verified in the sandbox on
Rust 1.100.

## Fixes and Problems

- None yet from this pass — the design (mechanism in `gindema-vm`,
  randomness policy here, `Program` self-describing its own `OpcodeMap` so
  nothing extra needs tracking) worked on the first implementation attempt,
  verified by the "remap and run both ways" tests in `opcode_remap.rs`.
