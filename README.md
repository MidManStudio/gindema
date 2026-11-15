# gindema

Closed-source code/binary protection core **and** gameplay video/image poisoning tool, in one workspace, distributed as public CLIs. Blocks AI-assisted decompilation and reverse engineering, and disrupts AI vision models that would otherwise clone a game from posted gameplay footage.

## Status

Scaffold stage for `gindema-*` and `mirage-*`: crate structure is real, the
logic inside each isn't written yet. `pixelflip-*` is real and working.

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
- `crates/gindema-ptr` - vendored pointer primitives
- `crates/gindema-bits` - vendored bit-level I/O
- `crates/gindema-cli` - the distributable protection-core CLI

Video/image poisoning (placeholder name "mirage", rename before shipping):
- `crates/mirage-core` - perturbation algorithms
- `crates/mirage-ffmpeg` - FFmpeg integration
- `crates/mirage-train` - offline PGD-based training step that produces the perturbation pattern
- `crates/mirage-cli` - the distributable video-poisoning CLI

Visual effects (unrelated to the anti-AI purpose, just a styling tool):
- `crates/pixelflip-core` - pattern-based pixel negation effects
- `crates/pixelflip-cli` - the distributable effects CLI

## License

Proprietary, MidManStudio. See `LICENSE`.
