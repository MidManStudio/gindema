# Changelog

## [Unreleased]

- **`gindema-cli` gained `flatten`/`fluff` subcommands**, wiring up
  `gindema-obfuscate`'s `flatten_control_flow`/`obfuscate_with_fluff` (the
  previous entry below) so they can actually be run on a real program file
  without writing Rust — this project's whole workflow has no local
  toolchain to do that with otherwise. Both take the exact
  `<input> <output> --seed <S>` shape `remap` already used; `remap`'s own
  argument parser was generalized into a shared `parse_input_output_seed`
  rather than copy-pasted twice more. Documented, and demonstrated by hand
  in the sandbox: `flatten` must come before `fluff`/`remap` in a pipeline
  or it silently discards whichever ran first (still correct, just not
  what was probably intended) — run for real on a `demo --honeypots 1`
  program through the full recommended order
  (`flatten` → `fluff` → `remap` → `run`, `Int(55)` at every stage) and
  through the wrong order too, confirming the discarded-fluff behavior is
  real and not just a documentation claim. `gindema-cli`: 33 unit tests
  (up from 25: 4 new in `cli.rs`, 4 new in `commands.rs`).
- **`gindema-obfuscate`'s last two passes are implemented: control-flow
  flattening and state-dependent decoding.** All four of this crate's
  planned passes now exist.
  - `gindema-vm` gained the infrastructure both needed: `decode_program`
    (`decode.rs`) decodes an already-built `Program` back into a
    structured instruction list — closes part of the former "no
    disassembler" known limit, and is genuinely reusable beyond
    flattening alone. `interleave_fluff` (`fluff.rs`) is state-dependent
    decoding's actual bytecode-format mechanism: interleaves pseudo-random
    junk bits before every instruction, in an amount derived purely from a
    seed and that instruction's own bit *position* (deliberately not
    instruction *index* — see that module's docs for why the index-based
    design breaks the moment a `Jump` lands somewhere other than "the next
    instruction in program order"), and rewrites every jump target
    accordingly. `Program` gained a `fluff_seed: Option<u64>` field;
    `remap_program` was made fluff-aware (reads and copies existing fluff
    through unchanged via a new shared `copy_fluff` helper, rather than
    misreading it as a bogus opcode) so opcode remap and fluff compose in
    either order. **Breaking file-format change:** the on-disk format
    version bumped 1 → 2 for the new field; a version-1 `.gvm` file is now
    rejected with `SerializeError::UnsupportedVersion(1)` rather than
    silently assumed unfluffed, since the two cases can't be told apart
    from a version-1 file's bytes alone. `gindema-vm`: 75 unit tests + 2
    doctests (up from 58 + 2: 17 new, across `decode.rs`, `fluff.rs`, and
    `serialize.rs`'s format-change coverage).
  - `gindema-obfuscate` gained `flatten_control_flow` (`flatten.rs`):
    decodes a program into basic blocks (via `gindema_vm::decode_program`)
    and rebuilds it as a single dispatcher loop, so every inter-block
    transfer — including an ordinary block's own fallthrough into the next
    one — reads as "set a state local, jump back to the dispatcher"
    instead of a direct jump wherever the destination sits. Reserves one
    extra local slot for the dispatcher's own state (`ObfuscateError::
    TooManyLocals` if the input already uses all 255 a `u8` allows, rather
    than overflowing). The one residual direct branch `JumpIfFalse`
    still needs (no select/ternary opcode exists to avoid it) and the
    dispatcher's unchecked last case are both documented known limits, not
    hidden. `obfuscate_with_fluff` (`fluff.rs`) is a thin policy wrapper
    over `gindema_vm::interleave_fluff` — there's no permutation to choose
    for this pass, unlike opcode remap's real policy job. `lib.rs` gained
    this crate's first composability tests, chaining all four passes
    (honeypot → flatten → fluff → remap) end to end, plus a deliberate
    out-of-order case confirming flatten-after-fluff is still *correct*
    even though it silently discards the fluff (flattening always rebuilds
    clean via `ProgramBuilder`, documented in both crates' docs as the
    reason flatten belongs first in a pipeline). 22 unit tests + 4
    doctests (up from 11 + 2).
  - Both new mechanisms are sandbox-verified only so far — added after the
    commit `f8e0115` real-CI confirmation below, not yet covered by a real
    GitHub Actions run of their own.
- **CI confirmed for real.** `ci.yml` was run on the project's actual
  GitHub Actions for the first time — previously every crate in this
  workspace, `gindema-container` included, had only ever been
  sandbox-verified. rustc 1.100.0, commit `f8e0115`: `cargo test
  --workspace` 189 passed / 0 failed / 0 ignored, `cargo clippy --workspace
  -- -D warnings` and `cargo fmt --check` both clean.
- `gindema-vm`: added `assemble(&str) -> Result<Program, AsmError>`
  (`asm.rs`) — a minimal textual assembly language. Directly closes the
  gap both `docs/gindema-vm.md` and `docs/gindema-cli.md` flagged as the
  single biggest one in the workspace: `ProgramBuilder`'s Rust API was
  previously the only way to construct a `Program` at all. One
  instruction per line, `;` comments, an optional `.locals N` directive,
  `name:` labels (every label name is collected before any instruction is
  emitted, so a forward reference resolves exactly like a backward one),
  and the same 17 mnemonics as `ProgramBuilder`'s own emitter methods,
  one-to-one. 19 new unit tests + 1 new doctest (58 + 2 total, up from
  39 + 1).
- `gindema-cli`: wired up both `gindema-vm`'s new assembler and
  `gindema-container`, closing two of the three gaps
  `docs/gindema-cli.md`'s "Known limits" previously named. New
  subcommands: `assemble <source> <output>` (compiles a `.gasm` text file
  via `gindema_vm::assemble` — the first way to get an arbitrary program
  from the command line; `demo` remains the fixed hardcoded one);
  `pack <output> <input>... [--honeypots N] [--seed S]` (packs one or more
  files into a new `gindema-container` archive, generating and printing a
  fresh key — there is no other way to recover it, so it must be saved
  now); `unpack <input> --key <BASE64> <output-dir>` (opens an archive and
  extracts every real asset, named by 0-based order since the container
  stores no filenames by design — see `docs/gindema-container.md`).
  `pack_assets`/`unpack_all` kept as pure, filesystem-free functions,
  mirroring `build_demo_program`'s existing separation of testable logic
  from I/O. 10 new unit tests (25 total, up from 15); the built binary was
  also run end-to-end by hand — `assemble` → `info`/`run` → `remap` →
  `info`/`run` again on a real loop-sum-to-10 program (not just
  straight-line arithmetic), `pack` → `unpack` with the right key, and
  `unpack` with a wrong key (both a malformed one and a well-formed-but-
  incorrect one, to exercise both of `ContainerReader::open`'s failure
  paths) — not just `cargo test`. `gindema-guard` and `gindema-ptr` remain
  the only declared-but-unused dependencies.
- `gindema-common`: gained `SplitMix64` (moved from `gindema-obfuscate`,
  made public, `fill_bytes` added) once `gindema-container` needed the
  identical seeded generator rather than a third copy in the workspace.
  `gindema-obfuscate` updated to call `gindema_common::SplitMix64` instead
  of its own now-deleted copy. 9 unit tests (up from 4).
- `gindema-container`: **implemented** — no longer a scaffold stub.
  Encrypted asset archive on top of `dixscript`'s real `Aes256Encryptor`/
  `GzipCompressor` (used directly via their `IEncryptor`/`ICompressor`
  traits, bypassing `dixscript`'s AST-driven `DLMPipelineExecutor`, which
  is built for compiling `.dix` script files rather than encrypting
  arbitrary bytes — see `docs/gindema-container.md`). `ContainerKey`
  (generation via dixscript's own CSPRNG-backed path, base64 storage),
  `ContainerBuilder` (add assets, build with seeded honeypot slots that
  are indistinguishable from real ones without the key and sized to blend
  with real asset sizes), `ContainerReader` (opens by decrypting only a
  small index; `read_asset` decrypts exactly one slot, not the whole
  archive — the "lazy per-asset decrypt" from the original scaffold note,
  now real). No password-based key derivation, no algorithm choice beyond
  AES-256-GCM, no per-chunk splitting of a single large asset — see that
  doc's "Known limits" for the full, honest list. 18 unit tests + 1
  doctest. **Building and testing this crate means `dixscript`'s own
  encryption/compression code actually ran in the sandbox** — a deliberate
  departure from this project's standing "don't execute DixScript-Rust"
  practice, since a wrapper this deep into another crate's crypto API
  can't be verified by reading alone; flagged prominently rather than done
  quietly. Still excluded from the broader `--workspace` clippy/test
  sweep, same as always. **This crate has not yet been run through the
  project's real GitHub Actions workflow** — sandbox-only so far.

- `gindema-common`: first real content. `checksum()`/`checksums_match()`
  (FNV-1a, 64-bit) moved here from `gindema-guard` once a second plausible
  consumer made "every crate rolls its own" the wrong call. `gindema-guard`
  updated to call `gindema_common::checksum` instead of its own now-deleted
  copy — finally gives this crate's long-unused dependency in
  `gindema-guard`'s `Cargo.toml` something real to do. Shared error
  types/traits (named in this crate's own description) still don't exist;
  see `docs/gindema-common.md`'s "Known limits" for why. 4 unit tests.
- `gindema-vm`: **bug fix.** `remap_program` stopped decoding at the first
  `Halt` opcode, which silently truncated any program containing a
  `gindema-obfuscate` honeypot (whose dead-end block has its own `Halt`,
  positioned earlier in the raw byte stream than the program's real one,
  even though never actually reached at runtime). Fixed by adding
  `Program::code_bit_len` — the exact real bit length, already computed by
  `ProgramBuilder::build`'s label-resolution pass but previously
  discarded — and having `remap_program` decode exactly that many bits
  instead. Caught by `gindema-cli`'s own integration test, not by any
  single crate's unit tests in isolation; see that crate's "Fixes and
  Problems" for the full story.
- `gindema-vm`: added `Program::to_bytes`/`from_bytes` — a self-contained
  binary file format (magic, version, `local_count`, `code_bit_len`, the
  opcode table, the constant pool, the bytecode) that makes a `Program`
  persistable at all; without it a program only ever existed for one
  process's lifetime. 39 unit tests + 1 doctest (up from 31 + 1).
- `gindema-obfuscate`: no functional change, but gained a 4th dependency
  edge worth noting — `gindema-cli` calling `inject_honeypot` followed by
  `remap_opcodes` is what surfaced the `gindema-vm` bug above.
- `gindema-cli`: first real pass — no longer a scaffold stub. `demo`
  (builds a fixed demo program, sums `1..=10` via a real loop, with
  optional honeypots), `info` (summarizes a program file), `run` (loads
  and runs one), `remap` (per-build opcode remap on a saved file) — a
  real, working pipeline. Hand-rolled argument parsing rather than `clap`,
  kept deliberately separate from command execution so it's unit-testable
  without touching the filesystem. Does not use `gindema-guard` or
  `gindema-container` — `docs/gindema-cli.md` explains why neither fits
  this crate's actual job rather than being an oversight. The single
  biggest gap: there is still no assembly language, so `demo`'s one fixed
  program is the only way to get a program file at all. 15 unit tests,
  plus the built binary itself run end-to-end by hand (`demo` →
  `info`/`run` → `remap` → `info`/`run` again → a deliberately-missing-file
  error case), not just `cargo test`.

- `gindema-ptr`: the open MIT/Apache-2.0 attribution question raised when
  this crate was first vendored is resolved — the project owner has
  decided the whole repository is treated as proprietary and no separate
  attribution/NOTICE file will be added. `docs/gindema-ptr.md` updated to
  record the decision rather than leave it flagged as open.
- `gindema-obfuscate`: second pass, honeypot/dead-end injection.
  `inject_honeypot(&mut ProgramBuilder, seed)` emits an always-taken
  conditional branch (two equal-valued constants, `Eq`/`Not`/
  `JumpIfFalse`) guarding a short run of fake arithmetic that ends in its
  own `Halt` — a literal dead end, never reached. Needed no changes to
  `gindema-vm` at all, unlike opcode remap — built entirely on
  `ProgramBuilder`'s existing label/jump API. Control-flow flattening and
  state-dependent decoding remain unimplemented; `docs/gindema-obfuscate.md`
  now also spells out the honeypot's own real weakness (the always-taken
  pattern is a fixed, recognizable shape once someone's read this source).
  11 unit tests + 2 doctests (up from 7 + 1).

- `gindema-guard`: first real pass. `checksum()` (FNV-1a 64-bit, explicitly
  non-cryptographic) and `Watch` (records a region's checksum, re-checks it
  later); `TimingWindow` (elapsed-time checks, the debugger/breakpoint
  signal); `GuardState` (one shared, sticky, one-directional pollution
  flag — no way to un-pollute); `Guarded<T>` (reads a decoy instead of the
  real value once polluted, rather than crashing or otherwise making
  detection observable). No unsafe code; removed the scaffold's unused
  `#![allow(unsafe_code)]`, same reasoning as `gindema-vm`'s. Does **not**
  use `gindema-ptr` yet — `docs/gindema-guard.md`'s "Known limits" spells
  out why honestly (can't verify a machine-code-level benefit from this
  sandbox) rather than adding it on faith. Not wired into any other crate
  yet. 15 unit tests + 1 doctest.

- `gindema-vm`: added the mechanism for per-build opcode remap.
  `OpCode::ALL`/`OPCODE_COUNT` enumerate the instruction set;
  `OpcodeMap::identity()`/`from_permutation()` map a raw 6-bit wire value
  to/from a logical opcode; `Program` now carries the `OpcodeMap` it was
  encoded with (`ProgramBuilder::build` always produces `identity()`);
  `remap_program(&Program, &OpcodeMap)` re-encodes a program's stream under
  a different map as a pure, lossless, single-pass transcoding — every
  instruction keeps its exact bit width and position (operand width is
  fixed by the opcode, independent of numbering), so jump targets need no
  recomputation at all. `Vm::run` now decodes via `program.opcode_map()`
  instead of the fixed identity numbering. 31 unit tests + 1 doctest (up
  from 22+1).
- `gindema-obfuscate`: first real pass, per-build opcode remap.
  `random_opcode_map(seed)`/`remap_opcodes(program, seed)` generate a
  seeded pseudorandom permutation (`SplitMix64`, a partial Fisher-Yates
  shuffle — not cryptographic, documented as such) and call into
  `gindema-vm`'s new mechanism. Not yet wired into any build pipeline —
  this is a library function today, not something `gindema-cli` calls
  automatically. Control-flow flattening, state-dependent decoding, and
  honeypot injection remain unimplemented; see `docs/gindema-obfuscate.md`'s
  "Known limits" for the honest read on how strong this first pass actually
  is. 7 unit tests + 1 doctest.

- `gindema-ptr`: implemented. Vendored all 8 modules from mid-engine's
  `mid-ptr` (itself ported from Bevy's `bevy_ptr`, MIT/Apache-2.0):
  `Aligned`/`Unaligned`/`IsAligned`, `ConstNonNull`, `DebugEnsureAligned`,
  `Ptr`/`PtrMut`/`OwningPtr`, `MovingPtr` + its field-deconstruction macros,
  `ThinSlicePtr`, `UnsafeCellDeref`. `no_std`, zero dependencies, opted into
  `[lints] workspace = true` with the documented `#![allow(unsafe_code)]`
  escape hatch. Fixed two issues found in the upstream source while porting
  (a clippy-flagged un-elided lifetime, a stray duplicate closing brace) and
  updated the doctests' crate-path references from `mid_ptr` to
  `gindema_ptr`. Flags an open licensing question (MIT/Apache-2.0 attribution
  for a publicly-distributed proprietary binary) rather than resolving it —
  see `docs/gindema-ptr.md`. 20 unit tests + 4 doctests.
- `gindema-bits`: implemented, but not as originally scaffolded. mbfa's
  `bitreader.rs`/`bitwriter.rs` turned out to be mbfa's own LZ-token codec
  built on the published `bitstream_io` crate, not generic bit I/O — nothing
  there to vendor. Implemented a fresh, zero-dependency `BitWriter`/
  `BitReader` pair instead: arbitrary-width (1..=64 bit) field read/write,
  byte alignment, and `seek_to_bit` (for `gindema-vm`'s jump instructions).
  See `docs/gindema-bits.md` for the full "plan change" note. 13 unit tests.
- `gindema-vm`: implemented a real baseline (not the full obfuscation
  layer — that's `gindema-obfuscate`'s job). 17 invented opcodes, a `Value`
  type (`Int`/`Float`/`Bool`), a two-pass label-resolving `ProgramBuilder`
  (resolves jump targets without needing to back-patch already-written bits,
  since every instruction's encoded width is fixed by its opcode), and a
  safe interpreter loop with a step-limit safety net against runaway loops.
  No unsafe code in this pass; the scaffold's `#![allow(unsafe_code)]` was
  removed since nothing uses it yet. Depends on `gindema-bits` for the
  bytecode stream; doesn't use `gindema-ptr` yet (nothing consumes it until
  gindema-guard/gindema-obfuscate exist). 22 unit tests + 1 doctest,
  including a real backward-jump loop and a forward conditional branch.

- Scaffold: workspace and crate structure only, no implementation yet. Protection core and video-poisoning tool merged into one workspace.
- Added `pixelflip-core`/`pixelflip-cli`: real, working pattern-based pixel negation effects (checker, stripes, diagonal, radial, noise, luminance). Not part of the anti-AI tooling, a separate styling utility.
- Raised workspace `rust-version` from 1.75 to 1.100 (CI now builds on `stable`) and set `resolver = "3"`. Real CI runs showed 1.85 fails on `image`/`zip` (they need 1.88) and 1.88 fails inside `candle-core 0.11.1` (AVX-512 target features are only stable from 1.89, and it calls `__cpuid_count` from safe code, which needs a Rust newer than 1.92). See README.
- `mirage-core`: implemented the `MIRP` pattern format, validation, bilinear stretching and in-place application to RGB8 frames.
- `mirage-train`: implemented. Differentiable CLIP vision tower (candle-transformers' own is not differentiable: its fused LayerNorm silently cuts the graph), PGD training of a universal low-resolution perturbation under an L-infinity budget, deterministic seeding, held-out evaluation with random-pattern and JPEG-only controls, `train`/`eval` CLI. Dropped the `candle-transformers` dependency. Not yet measured against the real CLIP checkpoint.
- `mirage-train`: training now gives each image the window of the pattern its centre crop shows (aspect-aware, plus random scale/shift jitter), instead of assuming the pattern covers exactly the crop; `train` also evaluates in-sample (`--eval-train`) and prints `{"train":..,"heldout":..}`; `--legacy-full-window` keeps the old behaviour for comparison. Motivated by the first real run, where the trained pattern did not beat a random one on held-out images. `mirage-core` gained `interp_taps_window` (13 tests); `mirage-train` has 29 unit and 3 CLI tests.
- `mirage-train-bench.yml`: the opt-in training job now takes `images`, `holdout`, `steps` and `batch_size`, uses a first-N sample of COCO val2017, and runs an A/B of the legacy window against aspect-aware windows.
- `mirage-train-bench.yml` and `ci.yml` write a job summary and upload logs as artifacts; the bench workflow has an opt-in `run_training` input for a real train/eval pass.
- `mirage-core` and `mirage-train` opt into `[workspace.lints]` (no crate did before; Cargo 1.100 warns for all of them).
- Formatted `pixelflip-core` and `pixelflip-cli` with rustfmt (whitespace only); `ci.yml`'s `cargo fmt --check` would have failed on them.
