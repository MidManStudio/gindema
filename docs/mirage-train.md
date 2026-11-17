# mirage-train

## Overview
The offline training step that produces the perturbation pattern `mirage-core` applies at runtime. Run once (or whenever the pattern needs regenerating), offline, against a corpus of images; it is not shipped to players.

It looks for one low-resolution RGB pattern that, added to frames within an L-infinity budget, pushes the CLIP image embedding away from where the clean frame lands (a universal, untargeted perturbation). Projected gradient ascent on `1 - cosine(embedding(frame + pattern), embedding(frame))`, sign-gradient steps, pattern clipped to `[-epsilon, +epsilon]` after each step. While training, each image sees only the part of the pattern its centre crop would show, with a little random scale and shift, and Gaussian noise stands in for compression. Pure Rust on candle (`candle-core`, `candle-nn`), no Python.

**What is and isn't established.** The mechanics are tested: gradients reach the pixels, the budget holds, training lowers similarity on a toy model, windowed stretching equals the runtime math, runs repeat from a seed, the CLI works end to end. Against the real CLIP checkpoint only one small run exists (see "Results so far"): the optimiser clearly works on its own training images, but on held-out images that run showed no advantage over a random pattern. Whether a trained pattern generalises, survives real compression, or transfers to other vision-language models is open. The `train-smoke` job in `mirage-train-bench.yml` is how that gets measured; treat anything else as hypothesis. Also note the metric: shifting a CLIP embedding is a proxy, not proof that a video-to-game cloning pipeline is disrupted.

## Usage

```
mirage-train train --weights model.safetensors --images DIR --out pattern.mirp \
    [--preview preview.png] [--summary-md summary.md] [--epsilon 8] [--steps 200] \
    [--batch-size 4] [--step-size E/16] [--low-res 28] [--noise-std 2.5] [--jitter 0.1] \
    [--legacy-full-window] [--seed 0] [--holdout N] [--eval-train 8] [--jpeg-quality 75]
mirage-train eval  --weights model.safetensors --images DIR --pattern pattern.mirp [--jpeg-quality 75]
```

`--weights` is the stock Hugging Face `model.safetensors` of `openai/clip-vit-base-patch32` (the same file candle's own CLIP example loads, revision `refs/pr/15`). Budget, step size and noise are given in 8-bit levels (`--epsilon 8` is 8/255). `--holdout N` keeps the last N images (by file name) out of training and evaluates on them afterwards; `--eval-train N` also evaluates on the first N training images in the same deployed pipeline, so a gap between the two shows overfitting. `train` prints `{"train": <report|null>, "heldout": <report|null>}` as JSON on stdout and progress on stderr; `eval` prints one report. `--legacy-full-window` reproduces the earlier assumption that the pattern covers exactly the crop (for A/B comparison).

## Modules

### `lib.rs`
**What it does:** declares the modules below.

### `clip.rs`
**What it does:** the CLIP vision tower (patch conv, class/position embeddings, pre-LN, N pre-LN encoder layers with multi-head attention and a QuickGELU MLP, post-LN on the class token, visual projection), loading tensors under the stock checkpoint's names (`vision_model.*`, `visual_projection.weight`). The text tower is never loaded. Also `l2_normalize`, `normalize_pixels` (CLIP mean/std) and `random_weights` (deterministic random weights for tests and smoke runs).

**Why not candle-transformers' `ClipVisionTransformer`:** it cannot be trained against. Its `candle_nn::LayerNorm` takes a fused kernel on contiguous input that has no backward pass and silently detaches the graph. Verified directly: with the stock `LayerNorm`, `grads.get(input)` is `None`; with `layer_norm_slow` it is `Some`. The loss would never see the perturbation. This tower uses only ops with backward passes (`layer_norm_slow`, generic `softmax`, `sigmoid`, `matmul`, `conv2d`), and `train` fails loudly ("no gradient reached the perturbation") if the graph ever gets cut again.

### `pgd.rs`
**What it does:** `train` (the loop), `TrainConfig`, `TrainOutcome`, `mean_cosine`, `embed_all`, and the differentiable stretch `Upsampler`.

**Design notes:**
- Clean embeddings are computed once up front; each step only runs the perturbed batch.
- The low-resolution grid is stretched by `U_h * delta * U_w^T` with interpolation matrices built from `mirage_core::interp_taps_window`, one pair per image. candle's `upsample_bilinear2d` has no backward pass (verified: `backward()` returns "backward not supported for upsample_bilinear2d"), and `upsample_nearest2d` only handles whole-number scale factors and gives blocky patterns.
- Each image gets the window of the pattern that its crop shows (`data::CropWindow`), randomly scaled and shifted by up to `jitter` every step. Tests assert the windowed stretch equals centre-cropping the runtime-stretched pattern.
- The initial grid is uniform in `[-epsilon, +epsilon]`; step size decays linearly to 10% over the run.
- All randomness comes from `rng.rs`, because candle cannot seed its CPU generator.

### `data.rs`
**What it does:** lists images (sorted), decodes them, CLIP-style preprocessing (resize shorter side with Catmull-Rom, centre crop), `CropWindow` / `crop_window` (which part of a frame-wide pattern the crop shows; shares its geometry with `preprocess` so pixels and window cannot disagree), stacks into a tensor, renders a visible preview of a pattern.

### `eval.rs`
**What it does:** scores a pattern the way it would be deployed: apply with `mirage-core` to the full-resolution frame, optionally JPEG round-trip, then preprocess and embed like training images. Reports mean cosine to the clean embedding (lower = stronger) for: JPEG alone (baseline), trained pattern, trained pattern + JPEG, a random +-epsilon pattern of the same size and budget (control), random + JPEG. The controls keep the numbers honest: optimisation has to beat noise, and compression moves embeddings by itself. Reports with fewer than 20 images say so.

### `rng.rs`
**What it does:** SplitMix64 with uniform, Gaussian (Box-Muller) and shuffle helpers.

### `main.rs`
**What it does:** the CLI (`train`, `eval`). A hidden `--tiny` flag swaps in a toy architecture for smoke tests.

**Known limits and next steps:**
- Compression is modelled by Gaussian noise during training and measured with JPEG only. No H.264 round trip yet.
- One encoder (ViT-B/32). Whether a pattern transfers to other vision-language models is untested.
- A universal pattern needs enough training images; with a handful it can overfit to them.
- CPU only; a full ViT-B/32 forward and backward per step is slow but fine for a few hundred steps on small batches.

## Results so far

First real run (`train-smoke`, 11 COCO val2017 photos: 8 train, 3 held out, 60 steps, batch 4, 8/255, 28x28 grid, crop-only training, i.e. the legacy window):

| | Mean cosine to clean frame (lower = stronger) |
|---|---|
| Training set, crop pipeline, no JPEG: random start -> trained | 0.9619 -> 0.8139 |
| Held-out, JPEG q75 only (baseline) | 0.9836 |
| Held-out, trained pattern | 0.9493 |
| Held-out, trained pattern + JPEG q75 | 0.9328 |
| Held-out, random pattern, same budget | 0.9439 |
| Held-out, random pattern + JPEG q75 | 0.9207 |

Reading: the optimiser works (large drop on the images it trained on), but on three unseen images in the deployed pipeline the trained pattern was no better than a random one, and the difference is well inside what three images can produce. Two explanations fit and the run cannot tell them apart: overfitting to eight images, or the train/deploy mismatch (training added the pattern to the 224 crop, deployment stretches it over the whole 640x480 frame and the encoder sees the central ~75%). The aspect-aware windows, jitter, in-sample evaluation and larger sample added afterwards are meant to separate and fix these; their results are not in yet.

## CI and Workflows

`.github/workflows/ci.yml` (workflow_dispatch only) runs `cargo test/clippy/fmt` for the whole workspace, which includes this crate: 29 unit tests (RNG, preprocessing and crop windows, tower shape and gradient flow, windowed upsampler agreement with `mirage-core`, per-image windows, jitter bounds, training lowers similarity within the budget with and without windows, seed repeatability, input validation, evaluation numbers) and 3 CLI integration tests (`tests/cli_smoke.rs`: train then eval end to end including the legacy mode, rejected holdout, error reporting).

`.github/workflows/mirage-train-bench.yml` (workflow_dispatch only) builds, tests and lints `mirage-core` + `mirage-train` and reports timings and binary size in the job summary. With `run_training` set it downloads the real CLIP weights and a first-N sample of COCO val2017 (inputs: `images`, `holdout`, `steps`, `batch_size`), then trains twice on the same data and seed, A: legacy window, B: aspect-aware windows plus jitter, and publishes both evaluation tables (held-out and in-sample), the patterns and previews as artifacts.

## Fixes and Problems

- `mirage-train bench` #1 (1.85.1 runner) failed at dependency resolution: `image 0.25.10` and `zip 9.6.0` (via `candle-core`) both require rustc 1.88.
- `mirage-train bench` #2 (1.88 runner) resolved, then failed compiling `candle-core 0.11.1`: its `quantized/repack_x86.rs` uses AVX-512 / `avxvnni` target features and intrinsics (unstable before Rust 1.89) and calls `__cpuid_count` from safe code (only safe in a Rust newer than 1.92). Fixed by building on `stable` and raising the workspace `rust-version` to 1.100. See the root `README.md`.
- The planned dependency on `candle-transformers` was dropped: its CLIP vision tower is not differentiable (see `clip.rs`), and the dependency would have pulled in `tokenizers` for nothing.
- clippy (1.100) objected to float literals with excessive precision, to `x % y != 0`, and to `a >= x && a <= y`; fixed (`is_multiple_of`, `contains`).
- First real run: the trained pattern did not beat a random one on held-out images (see "Results so far"). Training now models the crop window per image with jitter, evaluation also reports in-sample numbers, and the bench workflow runs an A/B on a larger sample.
