# gindema

Closed-source code/binary protection core **and** gameplay video/image poisoning tool, in one workspace, distributed as public CLIs. Blocks AI-assisted decompilation and reverse engineering, and disrupts AI vision models that would otherwise clone a game from posted gameplay footage.

## Status

Scaffold stage for `gindema-*` and `mirage-*`: crate structure is real, the
logic inside each isn't written yet. `pixelflip-*` is real and working.

## MSRV: 1.85, not 1.75

Originally pinned to `rust-version = "1.75"` (`[workspace.package]`,
inherited from mid-engine's convention), but current crates.io dependencies
don't support that anymore: `image`, `clap`, `dixscript`, and `candle-core`
(needed for `mirage-train`) all pull in transitive dependencies that
require Cargo's `edition2024` feature, which needs Rust 1.85+. Confirmed by
actually running `cargo check --workspace` and `cargo test --workspace`
under both floors: 1.75 fails on all four, 1.85+ passes clean across the
whole workspace, `dixscript` included. Matches DixScript's own README badge
(1.85+), its `Cargo.toml` metadata field claiming 1.75 is stale.

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
