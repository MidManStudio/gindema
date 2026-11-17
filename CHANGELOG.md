# Changelog

## [Unreleased]

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
