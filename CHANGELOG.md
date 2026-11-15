# Changelog

## [Unreleased]

- Scaffold: workspace and crate structure only, no implementation yet. Protection core and video-poisoning tool merged into one workspace.
- Added `pixelflip-core`/`pixelflip-cli`: real, working pattern-based pixel negation effects (checker, stripes, diagonal, radial, noise, luminance). Not part of the anti-AI tooling, a separate styling utility.
- Raised workspace `rust-version` from 1.75 to 1.100 (CI now builds on `stable`) and set `resolver = "3"`. Real CI runs showed 1.85 fails on `image`/`zip` (they need 1.88) and 1.88 fails inside `candle-core 0.11.1` (AVX-512 target features are only stable from 1.89, and it calls `__cpuid_count` from safe code, which needs a Rust newer than 1.92). See README.
- Scaffolded `mirage-train`: offline training step (candle + candle-transformers CLIP vision encoder, PGD-style optimization, L-infinity budget, low-resolution-upsampled perturbation). Not implemented yet. `cargo test --workspace` in CI covers it now that CI builds on `stable`; a dedicated `mirage-train-bench.yml` workflow is scaffolded for the real training/benchmark run once it's implemented.
