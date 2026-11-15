# mirage-train

## Overview
The offline training step that produces the perturbation pattern mirage-core ships with at runtime. Not part of the distributable tooling — this is run once (or whenever the pattern needs regenerating), offline, against a training image corpus, not per-user and not at game-runtime.

Runs a PGD-style optimization loop against an open CLIP-family vision encoder (candle-transformers' `models::clip::vision_model::ClipVisionTransformer`), using candle-core's own autodiff (`Var` + `backward()`) rather than any Python ML stack. The perturbation is constrained to an L-infinity imperceptibility budget and generated at low resolution then upsampled, so it survives H.264/JPEG re-encoding on platforms like YouTube/TikTok instead of being erased by it.

## Modules

### `main.rs`
**What it does:** scaffold stub, not yet implemented.

## CI and Workflows

Basic build/test/clippy is covered by the workspace-wide `.github/workflows/ci.yml` (workflow_dispatch only) now that this crate is a workspace member and CI builds on `stable`.

A dedicated `.github/workflows/mirage-train-bench.yml` (workflow_dispatch only) is scaffolded separately for the real training/benchmark run, since that needs network access to pull CLIP weights and takes meaningfully longer than a unit-test pass. Right now it only builds and smoke-tests the crate on the real `stable` toolchain; see that file's TODO for what the actual benchmark job still needs.

## Fixes and Problems

- `mirage-train bench` #1 (1.85.1 runner) failed at dependency resolution: `image 0.25.10` and `zip 9.6.0` (via `candle-core`) both require rustc 1.88.
- `mirage-train bench` #2 (1.88 runner) resolved, then failed compiling `candle-core 0.11.1`: its `quantized/repack_x86.rs` uses AVX-512 / `avxvnni` target features and intrinsics (unstable before Rust 1.89) and calls `__cpuid_count` from safe code (only safe in a Rust newer than 1.92). Not behind a cargo feature, and `candle-core` declares no `rust-version`. Fixed by building on `stable` in both workflows and raising the workspace `rust-version` to 1.100. See the root `README.md`.
