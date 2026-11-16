// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "pgd.rs"
// ============================================================================

//! Projected gradient ascent for a universal perturbation.
//!
//! One low-resolution RGB grid `delta` is shared by every training image. Each
//! step adds it (bilinearly stretched) to a batch, runs the CLIP tower, and
//! moves `delta` along the sign of the gradient that lowers the cosine
//! similarity between the perturbed embedding and that image's clean
//! embedding, then clips `delta` back into `[-epsilon, +epsilon]`.
//!
//! Stretching is a matrix product `U_h * delta * U_w^T` with fixed
//! interpolation matrices built from `mirage_core::interp_taps`. candle's own
//! `upsample_bilinear2d` has no backward pass, and a matrix product both has
//! one and guarantees training and runtime stretch the pattern identically.
//! Interpolation weights are non-negative and sum to 1, so clipping the small
//! grid bounds the stretched pattern by the same epsilon.

use anyhow::{bail, Context, Result};
use candle_core::{Device, Tensor, Var, D};
use mirage_core::{interp_taps, Pattern, CHANNELS};

use crate::clip::{l2_normalize, normalize_pixels, ClipVision};
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
    /// Seed for initial pattern, batch order and noise.
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

/// Dense `(dst, src)` linear-interpolation matrix from `interp_taps`.
pub fn interp_matrix(src: usize, dst: usize, device: &Device) -> candle_core::Result<Tensor> {
    let mut m = vec![0f32; dst * src];
    for (i, tap) in interp_taps(src, dst).iter().enumerate() {
        m[i * src + tap.lo] += 1.0 - tap.w_hi;
        m[i * src + tap.hi] += tap.w_hi;
    }
    Tensor::from_vec(m, (dst, src), device)
}

/// Differentiable bilinear stretch of a `(1, 3, h, w)` grid to `(1, 3, S, S)`.
pub struct Upsampler {
    up_h: Tensor,
    up_w_t: Tensor,
}

impl Upsampler {
    /// Builds the interpolation matrices for a `low` x `low` grid stretched to
    /// `size` x `size`.
    pub fn new(low: usize, size: usize, device: &Device) -> candle_core::Result<Self> {
        Ok(Self {
            up_h: interp_matrix(low, size, device)?,
            up_w_t: interp_matrix(low, size, device)?.t()?.contiguous()?,
        })
    }

    /// Stretches `delta`; gradients flow back to it.
    pub fn apply(&self, delta: &Tensor) -> candle_core::Result<Tensor> {
        self.up_h
            .broadcast_matmul(delta)?
            .broadcast_matmul(&self.up_w_t)
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
/// `clean`, over every image, without noise.
pub fn mean_cosine(
    model: &ClipVision,
    images: &Tensor,
    clean: &Tensor,
    delta: &Tensor,
    up: &Upsampler,
    batch: usize,
) -> candle_core::Result<f32> {
    let n = images.dim(0)?;
    let delta_up = up.apply(delta)?;
    let mut total = 0f32;
    let mut start = 0;
    while start < n {
        let len = batch.max(1).min(n - start);
        let x = images.narrow(0, start, len)?;
        let e = clean.narrow(0, start, len)?;
        let adv = x.broadcast_add(&delta_up)?.clamp(0f32, 1f32)?;
        let feats = l2_normalize(&model.forward(&normalize_pixels(&adv)?)?)?;
        total += (&feats * &e)?.sum_all()?.to_scalar::<f32>()?;
        start += len;
    }
    Ok(total / n as f32)
}

/// Trains a universal perturbation against `model` on `images`
/// `(N, 3, S, S)` in `[0, 1]`. `on_step` is called after every step.
pub fn train(
    model: &ClipVision,
    images: &Tensor,
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
    let device = images.device().clone();
    let low = cfg.low_res;
    let batch = cfg.batch_size.min(n);
    let up = Upsampler::new(low, size, &device)?;
    let clean = embed_all(model, images, batch)?;

    let mut rng = Rng::new(cfg.seed);
    let init: Vec<f32> = (0..CHANNELS * low * low)
        .map(|_| rng.uniform(-cfg.epsilon, cfg.epsilon))
        .collect();
    let delta = Var::from_tensor(&Tensor::from_vec(init, (1, CHANNELS, low, low), &device)?)?;
    let initial_mean_cos = mean_cosine(model, images, &clean, delta.as_tensor(), &up, batch)?;

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
            picked.push(order[cursor] as u32);
            cursor += 1;
        }
        let idx = Tensor::from_vec(picked, batch, &device)?;
        let x = images.index_select(&idx, 0)?;
        let e = clean.index_select(&idx, 0)?;

        let delta_up = up.apply(delta.as_tensor())?;
        let mut adv = x.broadcast_add(&delta_up)?.clamp(0f32, 1f32)?;
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

    let final_mean_cos = mean_cosine(model, images, &clean, delta.as_tensor(), &up, batch)?;
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

    #[test]
    fn matrix_stretch_matches_the_runtime_upsampler() {
        let low = 5;
        let size = 32;
        let mut rng = Rng::new(11);
        let grid: Vec<f32> = (0..3 * low * low)
            .map(|_| rng.uniform(-0.03, 0.03))
            .collect();
        let tensor = Tensor::from_vec(grid.clone(), (1, 3, low, low), &Device::Cpu).unwrap();
        let stretched = Upsampler::new(low, size, &Device::Cpu)
            .unwrap()
            .apply(&tensor)
            .unwrap()
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();

        let pattern = grid_to_pattern(&grid, low, low, 0.03).unwrap();
        let runtime = pattern.upsample(size, size);
        for c in 0..3 {
            for y in 0..size {
                for x in 0..size {
                    let a = stretched[c * size * size + y * size + x];
                    let b = runtime[(y * size + x) * 3 + c];
                    assert!(
                        (a - b).abs() < 1e-5,
                        "mismatch at c={c} y={y} x={x}: {a} vs {b}"
                    );
                }
            }
        }
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

    #[test]
    fn training_lowers_similarity_within_the_budget() {
        let model = tiny_model(21);
        let images = synthetic_images(6, 32, 4);
        let cfg = TrainConfig {
            epsilon: 0.1,
            steps: 30,
            batch_size: 3,
            step_size: 0.02,
            low_res: 4,
            noise_std: 0.0,
            seed: 9,
        };
        let mut seen = 0;
        let out = train(&model, &images, &cfg, |_| seen += 1).unwrap();
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
    fn training_is_repeatable_from_a_seed() {
        let model = tiny_model(21);
        let images = synthetic_images(4, 32, 4);
        let cfg = TrainConfig {
            epsilon: 0.05,
            steps: 5,
            batch_size: 2,
            step_size: 0.01,
            low_res: 4,
            noise_std: 0.01,
            seed: 3,
        };
        let a = train(&model, &images, &cfg, |_| {}).unwrap();
        let b = train(&model, &images, &cfg, |_| {}).unwrap();
        assert_eq!(a.pattern, b.pattern);
    }

    #[test]
    fn rejects_bad_inputs() {
        let model = tiny_model(21);
        let images = synthetic_images(2, 32, 4);
        let good = TrainConfig {
            low_res: 4,
            steps: 1,
            ..TrainConfig::default()
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
            assert!(train(&model, &images, &bad, |_| {}).is_err());
        }
        let wrong_size = synthetic_images(2, 16, 4);
        assert!(train(&model, &wrong_size, &good, |_| {}).is_err());
    }
}
