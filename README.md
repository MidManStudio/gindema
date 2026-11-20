# gindema

Closed-source code/binary protection core **and** gameplay video/image poisoning tool, in one workspace, distributed as public CLIs. Blocks AI-assisted decompilation and reverse engineering, and disrupts AI vision models that would otherwise clone a game from posted gameplay footage.

## Status

Protection core: `gindema-ptr` (vendored type-erased pointer primitives),
`gindema-bits` (bit-level stream I/O — see `docs/gindema-bits.md` for why
this ended up written fresh rather than literally vendored), and
`gindema-vm` (the baseline bytecode interpreter, now with an
`OpcodeMap`/`remap_program` mechanism for per-build remapping) are
implemented and tested. `gindema-obfuscate` has two of its four passes — per-build opcode remap
and honeypot/dead-end injection, see `docs/gindema-obfuscate.md` (including
honest limits on how strong each one currently is) — but not yet
control-flow flattening or state-dependent decoding.
`gindema-guard` has its first real pass too — timing/checksum watchdog
plus pollution-state reads, see `docs/gindema-guard.md` for real limits —
but doesn't use `gindema-ptr` yet and isn't wired into anything.
`gindema-common`, `gindema-container`, and `gindema-cli` are still scaffold
stage: crate structure is real, the logic inside each isn't written yet.
`mirage-ffmpeg` and `mirage-cli` are also still scaffold stage. `pixelflip-*`
is real and working. `mirage-core` (pattern format and frame application)
and `mirage-train` (offline training and evaluation against CLIP) are
implemented and tested, but a trained pattern's effectiveness against the
real CLIP checkpoint has only been partly measured; see
`docs/mirage-train.md`.

## Toolchain: stable (1.100 at time of writing), not 1.75, 1.85 or 1.88

Originally pinned to `rust-version = "1.75"` (inherited from mid-engine's
convention). That floor has been raised three times, each time because a real
CI run said so:

- **1.85** (first guess, for the `edition2024` dependencies): `mirage-train
  bench` #1 failed at dependency resolution. `image 0.25.10` and `zip 9.6.0`
  (pulled in by `candle-core`) both require 1.88.
- **1.88**: `mirage-train bench` #2 resolved fine, then failed compiling
  `candle-core 0.11.1` itself. Its x86 quantized kernels
  (`quantized/repack_x86.rs`, not behind any cargo feature) use AVX-512 and
  `avxvnni` target features and intrinsics, which were unstable before Rust
  1.89, and call `__cpuid_count` from safe code, which only became a safe
  function in `stdarch` after the revision that shipped in Rust 1.92.
  `candle-core` declares no `rust-version`, so Cargo can't warn about any of
  this up front.
- **Now**: CI builds on `stable` and `[workspace.package] rust-version` is
  1.100, the version that was current when this was last verified. The real
  floor is somewhere above 1.92 and hasn't been bisected, since these ship as
  prebuilt binaries and nobody downstream builds them from source.

The earlier "verified clean at 1.85+" claim in this file was wrong and has
been removed. `resolver = "3"` is set, so Cargo prefers dependency versions
compatible with the declared `rust-version` instead of always taking the
newest.

## Workspace layout

Protection core:
- `crates/gindema-common` - shared types/errors (used workspace-wide)
- `crates/gindema-vm` - the custom bytecode VM
- `crates/gindema-obfuscate` - compiler passes
- `crates/gindema-guard` - runtime anti-tamper watchdog
- `crates/gindema-container` - encrypted asset container (built on dixscript)
- `crates/gindema-ptr` - vendored type-erased pointer primitives (from mid-engine's `mid-ptr`)
- `crates/gindema-bits` - bit-level stream I/O for the VM's bytecode format (not vendored after all — see `docs/gindema-bits.md`)
- `crates/gindema-cli` - the distributable protection-core CLI

Video/image poisoning (placeholder name "mirage", rename before shipping):
- `crates/mirage-core` - pattern format and frame application (runtime side)
- `crates/mirage-ffmpeg` - FFmpeg integration
- `crates/mirage-train` - offline PGD training and evaluation of the perturbation pattern against CLIP
- `crates/mirage-cli` - the distributable video-poisoning CLI

Visual effects (unrelated to the anti-AI purpose, just a styling tool):
- `crates/pixelflip-core` - pattern-based pixel negation effects
- `crates/pixelflip-cli` - the distributable effects CLI

## License

Proprietary, MidManStudio. See `LICENSE`.
