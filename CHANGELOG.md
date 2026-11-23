# Changelog

## [Unreleased]

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
