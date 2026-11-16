# mirage-core

## Overview
The runtime side of the poisoning tooling. Loads the perturbation pattern that `mirage-train` produced offline and applies it to frames with plain CPU arithmetic: no ML framework, no model weights, no GPU.

The pattern is a small low-resolution RGB grid of signed offsets in `[-epsilon, +epsilon]` (pixel units, 1.0 = full scale), bilinearly stretched over a frame and added to it. It is stored low-resolution on purpose: a low-frequency perturbation is what survives H.264/JPEG re-encoding, where per-pixel noise does not.

Earlier scaffold notes also floated per-pixel random-gradient flips and DCT-domain noise. Neither exists; random noise only appears as the control in `mirage-train`'s evaluation.

## Modules

### `lib.rs`
**What it does:** declares the `pattern` module and re-exports its public items.

### `pattern.rs`
**What it does:** the whole runtime: the `Pattern` type, its file format, and applying it to frames.

- `Pattern::new` validates size, epsilon, length, finiteness and the `[-epsilon, +epsilon]` bound; every other constructor goes through it, so a `Pattern` is always valid.
- File format `MIRP` v1, little-endian: magic `MIRP` (4 bytes), version `u16` (= 1), channels `u16` (= 3), width `u32`, height `u32`, epsilon `f32`, then `width * height * 3` `f32` values, row-major, interleaved RGB. Total `20 + 12 * width * height` bytes. `from_bytes` rejects bad magic, other versions, other channel counts, wrong lengths, NaN/inf and out-of-bound values.
- `interp_taps(src, dst)`: 1-D linear interpolation weights (half-pixel centres, clamped edges). All weights are non-negative and sum to 1, so a stretched pattern can never exceed the grid's own range, and clipping the small grid to epsilon bounds every stretched pixel by the same epsilon.
- `Pattern::upsample(w, h)`: the stretched pattern as floats.
- `Pattern::apply_rgb8(frame, w, h)`: adds the stretched pattern to an interleaved RGB8 frame in place, rounding to the nearest level and clamping to `0..=255`. A channel changes by at most `round(epsilon * 255)` levels.

**Design notes:** `mirage-train` builds its differentiable upsampling matrices from the same `interp_taps`, and a test there asserts the matrix path and `Pattern::upsample` agree, so training and runtime stretch the pattern identically.

**Known limits:**
- The pattern is stretched over the whole frame. CLIP-style preprocessing resizes the shorter side and centre-crops, so for non-square frames the encoder sees only the central part of the pattern. Training does not model this yet.
- `apply_rgb8` is a plain per-pixel bilinear loop: RGB8 only, no SIMD, no cached rows. Not integrated with `mirage-ffmpeg` yet.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, workflow_dispatch only). `cargo test -p mirage-core` runs 10 unit tests covering the format, validation, interpolation and application.

## Fixes and Problems

- clippy (1.100) flagged `chunks_exact(4)` with a constant chunk size (`chunks_exact_to_as_chunks`); `from_bytes` uses `as_chunks::<4>()` now.
- `[workspace.lints]` was never inherited: no crate in the workspace had `[lints] workspace = true`, and Cargo 1.100 warns about it for every member. `mirage-core` and `mirage-train` now opt in (and pass `clippy -D warnings` with them on); the other crates don't yet.
