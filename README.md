# gindema

Closed-source code/binary protection core **and** gameplay video/image poisoning tool, in one workspace, distributed as public CLIs. Blocks AI-assisted decompilation and reverse engineering, and disrupts AI vision models that would otherwise clone a game from posted gameplay footage.

## Status

Scaffold stage for `gindema-*` and `mirage-*`: crate structure is real, the
logic inside each isn't written yet. `pixelflip-*` is real and working.

## MSRV: 1.88, not 1.75 or 1.85

Originally pinned to `rust-version = "1.75"` (`[workspace.package]`,
inherited from mid-engine's convention). Current crates.io dependencies moved
past that: `image`, `clap`, `dixscript`, and `candle-core` (needed for
`mirage-train`) pull in transitive dependencies that require Cargo's
`edition2024` feature, which needs Rust 1.85+.

1.85 turned out not to be enough either. The first real CI run
(`mirage-train bench` #1, on a 1.85.1 runner) failed at dependency
resolution: `image 0.25.10` and `zip 9.6.0` (pulled in by `candle-core`) both
declare `rust-version` 1.88. So the floor is 1.88, and CI pins
`dtolnay/rust-toolchain@1.88` to match. The earlier "verified clean at 1.85+"
claim that used to be here did not hold up against a real runner and has been
removed.

The workspace also sets `resolver = "3"`, which makes Cargo prefer dependency
versions compatible with the declared `rust-version` instead of always taking
the newest, so a future release that raises its own MSRV past 1.88 doesn't
silently break the build.

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
