# Changelog

## [Unreleased]

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
