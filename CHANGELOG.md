# Changelog

## [Unreleased]

- Scaffold: workspace and crate structure only, no implementation yet. Protection core and video-poisoning tool merged into one workspace.
- Added `pixelflip-core`/`pixelflip-cli`: real, working pattern-based pixel negation effects (checker, stripes, diagonal, radial, noise, luminance). Not part of the anti-AI tooling, a separate styling utility.
- Bumped workspace `rust-version` from 1.75 to 1.88 and set `resolver = "3"`. Current `image`, `clap`, `dixscript`, and `candle-core` all need `edition2024` (1.85+), and a real CI run on 1.85.1 (`mirage-train bench` #1) still failed because `image 0.25.10` and `zip 9.6.0` (via `candle-core`) require 1.88. See README.
- Scaffolded `mirage-train`: offline training step (candle + candle-transformers CLIP vision encoder, PGD-style optimization, L-infinity budget, low-resolution-upsampled perturbation). Not implemented yet. `cargo test --workspace` in CI covers it now that the toolchain is 1.88; a dedicated `mirage-train-bench.yml` workflow is scaffolded for the real training/benchmark run once it's implemented.
