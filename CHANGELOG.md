# Changelog

## [Unreleased]

- Scaffold: workspace and crate structure only, no implementation yet. Protection core and video-poisoning tool merged into one workspace.
- Added `pixelflip-core`/`pixelflip-cli`: real, working pattern-based pixel negation effects (checker, stripes, diagonal, radial, noise, luminance). Not part of the anti-AI tooling, a separate styling utility.
- Bumped workspace `rust-version` from 1.75 to 1.85. Current `image`, `clap`, `dixscript`, and `candle-core` all require it (edition2024). Verified clean: `cargo check --workspace` and `cargo test --workspace` both pass under 1.85+, including `gindema-container`'s `dixscript` dependency. See README.
