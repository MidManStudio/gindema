// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "pgd.rs"
// ============================================================================

//! Projected gradient ascent for a universal perturbation.
//!
//! One low-resolution RGB grid `delta` is shared by every training image. Each
//! step adds it to a batch, runs the CLIP tower, and moves `delta` along the
//! sign of the gradient that lowers the cosine similarity between the
//! perturbed embedding and that image's clean embedding, then clips `delta`
//! back into `[-epsilon, +epsilon]`.
//!
//! The grid is stretched to the model's resolution by a matrix product
//! `U_h * delta * U_w^T`, with one pair of interpolation matrices per image.
//! Each image gets its own *window* of the grid: at runtime the pattern covers
//! the whole frame, but the encoder centre-crops, so for a non-square frame it
//! only sees part of it (`data::CropWindow`). Training adds a little random
//! scale and shift to that window so the pattern does not depend on exact
//! alignment. candle's own `upsample_bilinear2d` has no backward pass, and a
//! matrix product both has one and lets the weights come from
//! `mirage_core::interp_taps_window`, the same math the runtime uses.
//! Interpolation weights are non-negative and sum to 1, so clipping the small
//! grid bounds the stretched pattern by the same epsilon.

use anyhow::{bail, Context, Result};
use candle_core::{Device, Tensor, Var, D};
use mirage_core::{interp_taps_window, Pattern, CHANNELS};

use crate::clip::{l2_normalize, normalize_pixels, ClipVision};
use crate::data::CropWindow;
use crate::rng::Rng;

/// Training hyper-parameters. Pixel quantities are in `[0, 1]` units
/// (8 levels of 255 is `8.0 / 255.0`).
#[derive(Debug, Clone)]
pub struct TrainConfig {
    /// L-infinity budget of the pattern.
    pub epsilon: f32,
    /// Number of gradient steps.
    pub steps: usize,
    /// Images per step.
    pub batch_size: usize,
    /// Initial sign-gradient step size; decays linearly to 10% over the run.
    pub step_size: f32,
    /// Side of the trained grid (the pattern is `low_res` x `low_res`).
    pub low_res: usize,
    /// Standard deviation of Gaussian noise added to perturbed inputs while
    /// training, a cheap stand-in for compression noise.
    pub noise_std: f32,
    /// Random scale and shift applied to each image's pattern window every
    /// step, as a fraction of the window (0.1 = up to about 10%). 0 disables.
    pub jitter: f32,
    /// Seed for initial pattern, batch order, jitter and noise.
    pub seed: u64,
}

impl Default for TrainConfig {
    fn default() -> Self {
        let epsilon = 8.0 / 255.0;
        Self {
            epsilon,
            steps: 200,
            batch_size: 4,
            step_size: epsilon / 16.0,
            low_res: 28,
            noise_std: 0.01,
            jitter: 0.1,
            seed: 0,
        }
    }
}

impl TrainConfig {
    fn validate(&self) -> Result<()> {
        if !(self.epsilon.is_finite() && self.epsilon > 0.0 && self.epsilon <= 0.5) {
            bail!("epsilon must be in (0, 0.5], got {}", self.epsilon);
        }
        if !(self.step_size.is_finite() && self.step_size > 0.0) {
            bail!("step_size must be positive, got {}", self.step_size);
        }
        if !(self.noise_std.is_finite() && self.noise_std >= 0.0) {
            bail!("noise_std must be non-negative, got {}", self.noise_std);
        }
        if !(self.jitter.is_finite() && (0.0..=0.5).contains(&self.jitter)) {
            bail!("jitter must be in [0, 0.5], got {}", self.jitter);
        }
        if self.steps == 0 || self.batch_size == 0 || self.low_res == 0 {
            bail!("steps, batch_size and low_res must all be at least 1");
        }
        Ok(())
    }
}

/// Progress of one gradient step.
#[derive(Debug, Clone, Copy)]
pub struct StepLog {
    /// Zero-based step number.
    pub step: usize,
    /// `1 - mean cosine` on this step's (noisy) batch; training maximises it.
    pub loss: f32,
    /// Mean cosine similarity to the clean embeddings on this batch.
    pub mean_cos: f32,
    /// Step size used.
    pub step_size: f32,
}

/// Result of a training run.
#[derive(Debug, Clone)]
pub struct TrainOutcome {
    /// The trained pattern, ready for `mirage-core`.
    pub pattern: Pattern,
    /// Mean cosine to clean embeddings over the whole training set, noise-free,
    /// before the first step (random start).
    pub initial_mean_cos: f32,
    /// The same quantity after the last step. Lower is stronger.
    pub final_mean_cos: f32,
    /// One entry per step.
    pub history: Vec<StepLog>,
}

/// Differentiable stretch of a `(1, 3, low, low)` grid to `(B, 3, S, S)`, one
/// window of the grid per image.
pub struct Upsampler {
    low: usize,
    size: usize,
    device: Device,
}

impl Upsampler {
    /// Stretches a `low` x `low` grid to `size` x `size`.
    pub fn new(low: usize, size: usize, device: &Device) -> Self {
        Self {
            low,
            size,
            device: device.clone(),
        }
    }

    /// Stacked interpolation matrices: `(B, 1, S, low)` for rows and
    /// `(B, 1, low, S)` (already transposed) for columns.
    fn matrices(&self, windows: &[CropWindow]) -> candle_core::Result<(Tensor, Tensor)> {
        let (low, size, b) = (self.low, self.size, windows.len());
        let mut rows = vec![0f32; b * size * low];
        let mut cols = vec![0f32; b * low * size];
        for (n, w) in windows.iter().enumerate() {
            for (i, tap) in interp_taps_window(low, size, w.y0, w.y_extent)
                .iter()
                .enumerate()
            {
                rows[(n * size + i) * low + tap.lo] += 1.0 - tap.w_hi;
                rows[(n * size + i) * low + tap.hi] += tap.w_hi;
            }
            for (j, tap) in interp_taps_window(low, size, w.x0, w.x_extent)
                .iter()
                .enumerate()
            {
                cols[(n * low + tap.lo) * size + j] += 1.0 - tap.w_hi;
                cols[(n * low + tap.hi) * size + j] += tap.w_hi;
            }
        }
        Ok((
            Tensor::from_vec(rows, (b, 1, size, low), &self.device)?,
            Tensor::from_vec(cols, (b, 1, low, size), &self.device)?,
        ))
    }

    /// Stretches `delta` once per window; gradients flow back to `delta`.
    pub fn apply(&self, delta: &Tensor, windows: &[CropWindow]) -> candle_core::Result<Tensor> {
        let (rows, cols) = self.matrices(windows)?;
        rows.broadcast_matmul(delta)?.broadcast_matmul(&cols)
    }
}

/// Adds random scale and shift to one axis of a window, keeping it inside `[0, 1]`.
fn jitter_axis(start: f64, extent: f64, jitter: f32, rng: &mut Rng) -> (f64, f64) {
    let scale = 1.0 + f64::from(rng.uniform(-jitter, jitter));
    let new_extent = (extent * scale).clamp(0.05, 1.0);
    let shift = f64::from(rng.uniform(-jitter, jitter)) * extent;
    let centre = start + extent / 2.0 + shift;
    let new_start = (centre - new_extent / 2.0).clamp(0.0, 1.0 - new_extent);
    (new_start, new_extent)
}

/// A randomly scaled and shifted copy of `window`; unchanged when `jitter` is 0.
fn jittered(window: CropWindow, jitter: f32, rng: &mut Rng) -> CropWindow {
    if jitter <= 0.0 {
        return window;
    }
    let (x0, x_extent) = jitter_axis(window.x0, window.x_extent, jitter, rng);
    let (y0, y_extent) = jitter_axis(window.y0, window.y_extent, jitter, rng);
    CropWindow {
        x0,
        x_extent,
        y0,
        y_extent,
    }
}

/// CHW grid -> `mirage-core` pattern (row-major interleaved RGB).
fn grid_to_pattern(chw: &[f32], h: usize, w: usize, epsilon: f32) -> Result<Pattern> {
    let mut data = vec![0f32; h * w * CHANNELS];
    for c in 0..CHANNELS {
        for y in 0..h {
            for x in 0..w {
                data[(y * w + x) * CHANNELS + c] = chw[c * h * w + y * w + x];
            }
        }
    }
    Ok(Pattern::new(w, h, epsilon, data)?)
}

/// Unit-norm embeddings of `images` `(N, 3, S, S)` in `[0, 1]`, in chunks.
pub fn embed_all(model: &ClipVision, images: &Tensor, batch: usize) -> candle_core::Result<Tensor> {
    let n = images.dim(0)?;
    let mut parts = Vec::new();
    let mut start = 0;
    while start < n {
        let len = batch.max(1).min(n - start);
        let chunk = images.narrow(0, start, len)?;
        parts.push(l2_normalize(&model.forward(&normalize_pixels(&chunk)?)?)?);
        start += len;
    }
    Tensor::cat(&parts, 0)
}

/// Mean cosine between the embeddings of `images + stretched delta` and
/// `clean`, over every image, without noise or jitter.
pub fn mean_cosine(
    model: &ClipVision,
    images: &Tensor,
    clean: &Tensor,
    delta: &Tensor,
    windows: &[CropWindow],
    up: &Upsampler,
    batch: usize,
) -> candle_core::Result<f32> {
    let n = images.dim(0)?;
    let mut total = 0f32;
    let mut start = 0;
    while start < n {
        let len = batch.max(1).min(n - start);
        let x = images.narrow(0, start, len)?;
        let e = clean.narrow(0, start, len)?;
        let delta_up = up.apply(delta, &windows[start..start + len])?;
        let adv = (&x + &delta_up)?.clamp(0f32, 1f32)?;
        let feats = l2_normalize(&model.forward(&normalize_pixels(&adv)?)?)?;
        total += (&feats * &e)?.sum_all()?.to_scalar::<f32>()?;
        start += len;
    }
    Ok(total / n as f32)
}

/// Trains a universal perturbation against `model` on `images`
/// `(N, 3, S, S)` in `[0, 1]`. `windows[i]` is the part of a frame-wide
/// pattern that image `i`'s crop shows (`CropWindow::FULL` if the pattern is
/// to be treated as covering exactly the crop). `on_step` is called after
/// every step.
pub fn train(
    model: &ClipVision,
    images: &Tensor,
    windows: &[CropWindow],
    cfg: &TrainConfig,
    mut on_step: impl FnMut(&StepLog),
) -> Result<TrainOutcome> {
    cfg.validate()?;
    let (n, channels, h, w) = images.dims4()?;
    let size = model.image_size();
    if channels != 3 || h != size || w != size {
        bail!("images must be (N, 3, {size}, {size}), got ({n}, {channels}, {h}, {w})");
    }
    if n == 0 {
        bail!("no training images");
    }
    if windows.len() != n {
        bail!("{} images but {} crop windows", n, windows.len());
    }
    let device = images.device().clone();
    let low = cfg.low_res;
    let batch = cfg.batch_size.min(n);
    let up = Upsampler::new(low, size, &device);
    let clean = embed_all(model, images, batch)?;

    let mut rng = Rng::new(cfg.seed);
    let init: Vec<f32> = (0..CHANNELS * low * low)
        .map(|_| rng.uniform(-cfg.epsilon, cfg.epsilon))
        .collect();
    let delta = Var::from_tensor(&Tensor::from_vec(init, (1, CHANNELS, low, low), &device)?)?;
    let initial_mean_cos = mean_cosine(
        model,
        images,
        &clean,
        delta.as_tensor(),
        windows,
        &up,
        batch,
    )?;

    let mut order: Vec<usize> = (0..n).collect();
    rng.shuffle(&mut order);
    let mut cursor = 0;
    let mut history = Vec::with_capacity(cfg.steps);

    for step in 0..cfg.steps {
        let mut picked = Vec::with_capacity(batch);
        while picked.len() < batch {
            if cursor == order.len() {
                rng.shuffle(&mut order);
                cursor = 0;
            }
            picked.push(order[cursor]);
            cursor += 1;
        }
        let batch_windows: Vec<CropWindow> = picked
            .iter()
            .map(|&i| jittered(windows[i], cfg.jitter, &mut rng))
            .collect();
        let idx = Tensor::from_vec(picked.iter().map(|&i| i as u32).collect(), batch, &device)?;
        let x = images.index_select(&idx, 0)?;
        let e = clean.index_select(&idx, 0)?;

        let delta_up = up.apply(delta.as_tensor(), &batch_windows)?;
        let mut adv = (&x + &delta_up)?.clamp(0f32, 1f32)?;
        if cfg.noise_std > 0.0 {
            let noise: Vec<f32> = (0..batch * 3 * size * size)
                .map(|_| rng.gaussian() * cfg.noise_std)
                .collect();
            let noise = Tensor::from_vec(noise, (batch, 3, size, size), &device)?;
            adv = (&adv + &noise)?;
        }
        let feats = l2_normalize(&model.forward(&normalize_pixels(&adv)?)?)?;
        let mean_cos = (&feats * &e)?.sum(D::Minus1)?.mean_all()?;
        let loss = mean_cos.affine(-1.0, 1.0)?;

        let grads = loss.backward()?;
        let grad = grads.get(delta.as_tensor()).context(
            "no gradient reached the perturbation: the graph is detached somewhere \
             (a non-differentiable op in the model?)",
        )?;

        let progress = if cfg.steps > 1 {
            step as f32 / (cfg.steps - 1) as f32
        } else {
            0.0
        };
        let alpha = cfg.step_size * (1.0 - 0.9 * progress);
        let stepped = (delta.as_tensor() + (grad.sign()? * f64::from(alpha))?)?;
        delta.set(&stepped.clamp(-cfg.epsilon, cfg.epsilon)?)?;

        let log = StepLog {
            step,
            loss: loss.to_scalar::<f32>()?,
            mean_cos: mean_cos.to_scalar::<f32>()?,
            step_size: alpha,
        };
        on_step(&log);
        history.push(log);
    }

    let final_mean_cos = mean_cosine(
        model,
        images,
        &clean,
        delta.as_tensor(),
        windows,
        &up,
        batch,
    )?;
    let chw = delta.as_tensor().flatten_all()?.to_vec1::<f32>()?;
    Ok(TrainOutcome {
        pattern: grid_to_pattern(&chw, low, low, cfg.epsilon)?,
        initial_mean_cos,
        final_mean_cos,
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::testing::tiny_model;

    fn synthetic_images(n: usize, size: usize, seed: u64) -> Tensor {
        let mut rng = Rng::new(seed);
        let mut flat = Vec::with_capacity(n * 3 * size * size);
        for i in 0..n {
            let base = 0.2 + 0.6 * (i as f32 / n as f32);
            for _ in 0..3 * size * size {
                flat.push((base + 0.15 * (rng.next_f32() - 0.5)).clamp(0.0, 1.0));
            }
        }
        Tensor::from_vec(flat, (n, 3, size, size), &Device::Cpu).unwrap()
    }

    fn random_grid(low: usize, eps: f32, seed: u64) -> (Vec<f32>, Tensor) {
        let mut rng = Rng::new(seed);
        let grid: Vec<f32> = (0..3 * low * low).map(|_| rng.uniform(-eps, eps)).collect();
        let tensor = Tensor::from_vec(grid.clone(), (1, 3, low, low), &Device::Cpu).unwrap();
        (grid, tensor)
    }

    fn flat(t: &Tensor) -> Vec<f32> {
        t.flatten_all().unwrap().to_vec1::<f32>().unwrap()
    }

    #[test]
    fn full_window_stretch_matches_the_runtime_upsampler() {
        let (low, size) = (5, 32);
        let (grid, tensor) = random_grid(low, 0.03, 11);
        let stretched = flat(
            &Upsampler::new(low, size, &Device::Cpu)
                .apply(&tensor, &[CropWindow::FULL])
                .unwrap(),
        );
        let runtime = grid_to_pattern(&grid, low, low, 0.03)
            .unwrap()
            .upsample(size, size);
        for c in 0..3 {
            for y in 0..size {
                for x in 0..size {
                    let a = stretched[c * size * size + y * size + x];
                    let b = runtime[(y * size + x) * 3 + c];
                    assert!((a - b).abs() < 1e-5, "c={c} y={y} x={x}: {a} vs {b}");
                }
            }
        }
    }

    #[test]
    fn windowed_stretch_equals_cropping_the_runtime_pattern() {
        // A pattern stretched over a 32x32 frame, centre-cropped to 16x16 (window
        // 0.25..0.75 on both axes), must equal the windowed training stretch.
        let (low, size) = (5, 16);
        let (grid, tensor) = random_grid(low, 0.03, 12);
        let window = CropWindow {
            x0: 0.25,
            x_extent: 0.5,
            y0: 0.25,
            y_extent: 0.5,
        };
        let stretched = flat(
            &Upsampler::new(low, size, &Device::Cpu)
                .apply(&tensor, &[window])
                .unwrap(),
        );
        let frame = 32;
        let runtime = grid_to_pattern(&grid, low, low, 0.03)
            .unwrap()
            .upsample(frame, frame);
        for c in 0..3 {
            for y in 0..size {
                for x in 0..size {
                    let a = stretched[c * size * size + y * size + x];
                    let b = runtime[((y + 8) * frame + (x + 8)) * 3 + c];
                    assert!((a - b).abs() < 1e-5, "c={c} y={y} x={x}: {a} vs {b}");
                }
            }
        }
    }

    #[test]
    fn each_image_in_a_batch_gets_its_own_window() {
        let (low, size) = (4, 8);
        let (_, tensor) = random_grid(low, 0.03, 13);
        let narrow = CropWindow {
            x0: 0.3,
            x_extent: 0.4,
            y0: 0.1,
            y_extent: 0.8,
        };
        let up = Upsampler::new(low, size, &Device::Cpu);
        let both = up.apply(&tensor, &[CropWindow::FULL, narrow]).unwrap();
        assert_eq!(both.dims(), &[2, 3, size, size]);
        let both = flat(&both);
        let half = 3 * size * size;
        let full_alone = flat(&up.apply(&tensor, &[CropWindow::FULL]).unwrap());
        let narrow_alone = flat(&up.apply(&tensor, &[narrow]).unwrap());
        assert_eq!(&both[..half], &full_alone[..]);
        assert_eq!(&both[half..], &narrow_alone[..]);
        assert_ne!(full_alone, narrow_alone);
    }

    #[test]
    fn jitter_keeps_windows_valid_and_zero_jitter_is_identity() {
        let base = CropWindow {
            x0: 0.125,
            x_extent: 0.75,
            y0: 0.0,
            y_extent: 1.0,
        };
        let mut rng = Rng::new(5);
        assert_eq!(jittered(base, 0.0, &mut rng), base);
        let mut moved = false;
        for _ in 0..500 {
            let w = jittered(base, 0.3, &mut rng);
            moved |= w != base;
            for (start, extent) in [(w.x0, w.x_extent), (w.y0, w.y_extent)] {
                assert!((0.05..=1.0).contains(&extent), "{w:?}");
                assert!(start >= 0.0 && start + extent <= 1.0 + 1e-12, "{w:?}");
            }
        }
        assert!(moved);
        let again = |seed| {
            let mut r = Rng::new(seed);
            jittered(base, 0.2, &mut r)
        };
        assert_eq!(again(9), again(9));
    }

    #[test]
    fn grid_pattern_conversion_preserves_every_value() {
        let (h, w) = (3, 4);
        let chw: Vec<f32> = (0..3 * h * w).map(|i| i as f32 * 0.001).collect();
        let p = grid_to_pattern(&chw, h, w, 0.1).unwrap();
        for c in 0..3 {
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(p.data()[(y * w + x) * 3 + c], chw[c * h * w + y * w + x]);
                }
            }
        }
    }

    fn small_config() -> TrainConfig {
        TrainConfig {
            epsilon: 0.1,
            steps: 30,
            batch_size: 3,
            step_size: 0.02,
            low_res: 4,
            noise_std: 0.0,
            jitter: 0.0,
            seed: 9,
        }
    }

    #[test]
    fn training_lowers_similarity_within_the_budget() {
        let model = tiny_model(21);
        let images = synthetic_images(6, 32, 4);
        let windows = vec![CropWindow::FULL; 6];
        let mut seen = 0;
        let out = train(&model, &images, &windows, &small_config(), |_| seen += 1).unwrap();
        assert_eq!(seen, 30);
        assert_eq!(out.history.len(), 30);
        assert!(
            out.final_mean_cos < out.initial_mean_cos - 1e-3,
            "initial {} final {}",
            out.initial_mean_cos,
            out.final_mean_cos
        );
        assert!(out.pattern.data().iter().all(|v| v.abs() <= 0.1 + 1e-6));
        assert_eq!((out.pattern.width(), out.pattern.height()), (4, 4));
    }

    #[test]
    fn training_with_cropped_windows_and_jitter_still_works() {
        let model = tiny_model(21);
        let images = synthetic_images(6, 32, 4);
        let wide = CropWindow {
            x0: 0.125,
            x_extent: 0.75,
            y0: 0.0,
            y_extent: 1.0,
        };
        let windows = vec![wide; 6];
        let cfg = TrainConfig {
            jitter: 0.1,
            ..small_config()
        };
        let out = train(&model, &images, &windows, &cfg, |_| {}).unwrap();
        assert!(
            out.final_mean_cos < out.initial_mean_cos - 1e-3,
            "initial {} final {}",
            out.initial_mean_cos,
            out.final_mean_cos
        );
        assert!(out.pattern.data().iter().all(|v| v.abs() <= 0.1 + 1e-6));
    }

    #[test]
    fn training_is_repeatable_from_a_seed() {
        let model = tiny_model(21);
        let images = synthetic_images(4, 32, 4);
        let windows = vec![CropWindow::FULL; 4];
        let cfg = TrainConfig {
            epsilon: 0.05,
            steps: 5,
            batch_size: 2,
            step_size: 0.01,
            noise_std: 0.01,
            jitter: 0.1,
            seed: 3,
            ..small_config()
        };
        let a = train(&model, &images, &windows, &cfg, |_| {}).unwrap();
        let b = train(&model, &images, &windows, &cfg, |_| {}).unwrap();
        assert_eq!(a.pattern, b.pattern);
    }

    #[test]
    fn rejects_bad_inputs() {
        let model = tiny_model(21);
        let images = synthetic_images(2, 32, 4);
        let windows = vec![CropWindow::FULL; 2];
        let good = TrainConfig {
            steps: 1,
            ..small_config()
        };
        for bad in [
            TrainConfig {
                epsilon: 0.0,
                ..good.clone()
            },
            TrainConfig {
                epsilon: f32::NAN,
                ..good.clone()
            },
            TrainConfig {
                step_size: -1.0,
                ..good.clone()
            },
            TrainConfig {
                noise_std: -0.1,
                ..good.clone()
            },
            TrainConfig {
                jitter: -0.1,
                ..good.clone()
            },
            TrainConfig {
                jitter: 0.9,
                ..good.clone()
            },
            TrainConfig {
                steps: 0,
                ..good.clone()
            },
            TrainConfig {
                batch_size: 0,
                ..good.clone()
            },
            TrainConfig {
                low_res: 0,
                ..good.clone()
            },
        ] {
            assert!(train(&model, &images, &windows, &bad, |_| {}).is_err());
        }
        let wrong_size = synthetic_images(2, 16, 4);
        assert!(train(&model, &wrong_size, &windows, &good, |_| {}).is_err());
        assert!(train(&model, &images, &windows[..1], &good, |_| {}).is_err());
    }
}
