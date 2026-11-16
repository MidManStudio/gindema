// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "clip.rs"
// ============================================================================

//! The CLIP image encoder, written so gradients reach the input pixels.
//!
//! candle-transformers ships a CLIP vision tower, but it is not usable for
//! this: its `candle_nn::LayerNorm` takes a fused kernel on contiguous input
//! that has no backward pass, which silently cuts the graph, so the loss never
//! sees the perturbation. This module is the same architecture and the same
//! weight names (it loads the stock Hugging Face `model.safetensors` for
//! `openai/clip-vit-base-patch32`) built only from ops that do have backward
//! passes. The text tower is not needed and is never loaded.

use candle_core::{Device, Result, Tensor, D};
use candle_nn::{ops, Conv2d, Conv2dConfig, Linear, Module, VarBuilder};

/// Per-channel mean CLIP's preprocessing subtracts.
pub const CLIP_MEAN: [f32; 3] = [0.481_454_66, 0.457_827_5, 0.408_210_73];
/// Per-channel standard deviation CLIP's preprocessing divides by.
pub const CLIP_STD: [f32; 3] = [0.268_629_54, 0.261_302_6, 0.275_777_1];

/// Architecture hyper-parameters of the vision tower.
#[derive(Debug, Clone)]
pub struct ClipVisionConfig {
    /// Transformer width.
    pub embed_dim: usize,
    /// MLP hidden width.
    pub intermediate_size: usize,
    /// Number of encoder layers.
    pub num_layers: usize,
    /// Attention heads per layer.
    pub num_heads: usize,
    /// Output embedding width after the visual projection.
    pub projection_dim: usize,
    /// Input resolution (square).
    pub image_size: usize,
    /// Patch size of the stem convolution.
    pub patch_size: usize,
    /// LayerNorm epsilon.
    pub layer_norm_eps: f32,
}

impl ClipVisionConfig {
    /// `openai/clip-vit-base-patch32`.
    pub fn vit_base_patch32() -> Self {
        Self {
            embed_dim: 768,
            intermediate_size: 3072,
            num_layers: 12,
            num_heads: 12,
            projection_dim: 512,
            image_size: 224,
            patch_size: 32,
            layer_norm_eps: 1e-5,
        }
    }

    /// A toy configuration for tests; same structure, tiny tensors.
    pub fn tiny() -> Self {
        Self {
            embed_dim: 16,
            intermediate_size: 32,
            num_layers: 2,
            num_heads: 2,
            projection_dim: 8,
            image_size: 32,
            patch_size: 16,
            layer_norm_eps: 1e-5,
        }
    }

    fn num_positions(&self) -> usize {
        (self.image_size / self.patch_size).pow(2) + 1
    }
}

/// LayerNorm built from differentiable primitives only.
#[derive(Debug, Clone)]
struct LayerNorm {
    weight: Tensor,
    bias: Tensor,
    eps: f32,
}

impl LayerNorm {
    fn new(size: usize, eps: f32, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            weight: vb.get(size, "weight")?,
            bias: vb.get(size, "bias")?,
            eps,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        ops::layer_norm_slow(xs, &self.weight, &self.bias, self.eps)
    }
}

#[derive(Debug, Clone)]
struct Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    out_proj: Linear,
    num_heads: usize,
    head_dim: usize,
    scale: f64,
}

impl Attention {
    fn new(vb: VarBuilder, cfg: &ClipVisionConfig) -> Result<Self> {
        let d = cfg.embed_dim;
        let head_dim = d / cfg.num_heads;
        Ok(Self {
            q_proj: candle_nn::linear(d, d, vb.pp("q_proj"))?,
            k_proj: candle_nn::linear(d, d, vb.pp("k_proj"))?,
            v_proj: candle_nn::linear(d, d, vb.pp("v_proj"))?,
            out_proj: candle_nn::linear(d, d, vb.pp("out_proj"))?,
            num_heads: cfg.num_heads,
            head_dim,
            scale: (head_dim as f64).powf(-0.5),
        })
    }

    fn split_heads(&self, xs: &Tensor, batch: usize, seq: usize) -> Result<Tensor> {
        xs.reshape((batch, seq, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (batch, seq, dim) = xs.dims3()?;
        let merged = (batch * self.num_heads, seq, self.head_dim);
        let q = (self.q_proj.forward(xs)? * self.scale)?;
        let q = self.split_heads(&q, batch, seq)?.reshape(merged)?;
        let k = self
            .split_heads(&self.k_proj.forward(xs)?, batch, seq)?
            .reshape(merged)?;
        let v = self
            .split_heads(&self.v_proj.forward(xs)?, batch, seq)?
            .reshape(merged)?;
        let weights = ops::softmax(&q.matmul(&k.transpose(1, 2)?)?, D::Minus1)?;
        let out = weights
            .matmul(&v)?
            .reshape((batch, self.num_heads, seq, self.head_dim))?
            .transpose(1, 2)?
            .reshape((batch, seq, dim))?;
        self.out_proj.forward(&out)
    }
}

#[derive(Debug, Clone)]
struct Mlp {
    fc1: Linear,
    fc2: Linear,
}

impl Mlp {
    fn new(vb: VarBuilder, cfg: &ClipVisionConfig) -> Result<Self> {
        Ok(Self {
            fc1: candle_nn::linear(cfg.embed_dim, cfg.intermediate_size, vb.pp("fc1"))?,
            fc2: candle_nn::linear(cfg.intermediate_size, cfg.embed_dim, vb.pp("fc2"))?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let hidden = self.fc1.forward(xs)?;
        // QuickGELU: x * sigmoid(1.702 * x).
        let activated = (&hidden * ops::sigmoid(&(&hidden * 1.702f64)?)?)?;
        self.fc2.forward(&activated)
    }
}

#[derive(Debug, Clone)]
struct EncoderLayer {
    layer_norm1: LayerNorm,
    self_attn: Attention,
    layer_norm2: LayerNorm,
    mlp: Mlp,
}

impl EncoderLayer {
    fn new(vb: VarBuilder, cfg: &ClipVisionConfig) -> Result<Self> {
        Ok(Self {
            layer_norm1: LayerNorm::new(cfg.embed_dim, cfg.layer_norm_eps, vb.pp("layer_norm1"))?,
            self_attn: Attention::new(vb.pp("self_attn"), cfg)?,
            layer_norm2: LayerNorm::new(cfg.embed_dim, cfg.layer_norm_eps, vb.pp("layer_norm2"))?,
            mlp: Mlp::new(vb.pp("mlp"), cfg)?,
        })
    }

    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let attended = self.self_attn.forward(&self.layer_norm1.forward(xs)?)?;
        let xs = (xs + attended)?;
        let mixed = self.mlp.forward(&self.layer_norm2.forward(&xs)?)?;
        &xs + mixed
    }
}

/// The CLIP vision tower plus visual projection: normalised pixels in,
/// un-normalised image embedding out.
#[derive(Debug, Clone)]
pub struct ClipVision {
    image_size: usize,
    patch_embedding: Conv2d,
    class_embedding: Tensor,
    position_embedding: Tensor,
    pre_layernorm: LayerNorm,
    layers: Vec<EncoderLayer>,
    post_layernorm: LayerNorm,
    visual_projection: Linear,
}

impl ClipVision {
    /// Loads the tower from a `VarBuilder` rooted at the checkpoint's top level
    /// (so tensors are looked up as `vision_model.*` and `visual_projection.*`).
    pub fn new(vb: VarBuilder, cfg: &ClipVisionConfig) -> Result<Self> {
        if cfg.num_heads == 0 || !cfg.embed_dim.is_multiple_of(cfg.num_heads) {
            candle_core::bail!("embed_dim must be a positive multiple of num_heads");
        }
        if cfg.patch_size == 0 || !cfg.image_size.is_multiple_of(cfg.patch_size) {
            candle_core::bail!("image_size must be a positive multiple of patch_size");
        }
        let vm = vb.pp("vision_model");
        let emb = vm.pp("embeddings");
        let patch_cfg = Conv2dConfig {
            stride: cfg.patch_size,
            ..Default::default()
        };
        let layers_vb = vm.pp("encoder").pp("layers");
        let layers = (0..cfg.num_layers)
            .map(|i| EncoderLayer::new(layers_vb.pp(i), cfg))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            image_size: cfg.image_size,
            patch_embedding: candle_nn::conv2d_no_bias(
                3,
                cfg.embed_dim,
                cfg.patch_size,
                patch_cfg,
                emb.pp("patch_embedding"),
            )?,
            class_embedding: emb.get(cfg.embed_dim, "class_embedding")?,
            position_embedding: emb
                .pp("position_embedding")
                .get((cfg.num_positions(), cfg.embed_dim), "weight")?,
            pre_layernorm: LayerNorm::new(
                cfg.embed_dim,
                cfg.layer_norm_eps,
                vm.pp("pre_layrnorm"),
            )?,
            layers,
            post_layernorm: LayerNorm::new(
                cfg.embed_dim,
                cfg.layer_norm_eps,
                vm.pp("post_layernorm"),
            )?,
            visual_projection: candle_nn::linear_no_bias(
                cfg.embed_dim,
                cfg.projection_dim,
                vb.pp("visual_projection"),
            )?,
        })
    }

    /// Input resolution the tower expects.
    pub fn image_size(&self) -> usize {
        self.image_size
    }

    /// `(batch, 3, S, S)` CLIP-normalised pixels -> `(batch, projection_dim)`.
    pub fn forward(&self, pixels: &Tensor) -> Result<Tensor> {
        let batch = pixels.dim(0)?;
        let dim = self.class_embedding.dim(0)?;
        let patches = self
            .patch_embedding
            .forward(pixels)?
            .flatten_from(2)?
            .transpose(1, 2)?;
        let class = self
            .class_embedding
            .reshape((1, 1, dim))?
            .broadcast_as((batch, 1, dim))?;
        let mut hidden =
            Tensor::cat(&[class, patches], 1)?.broadcast_add(&self.position_embedding)?;
        hidden = self.pre_layernorm.forward(&hidden)?;
        for layer in &self.layers {
            hidden = layer.forward(&hidden)?;
        }
        let pooled = hidden.narrow(1, 0, 1)?.squeeze(1)?;
        self.visual_projection
            .forward(&self.post_layernorm.forward(&pooled)?)
    }
}

/// Scales rows to unit L2 norm (differentiable).
pub fn l2_normalize(xs: &Tensor) -> Result<Tensor> {
    let norm = xs
        .sqr()?
        .sum_keepdim(D::Minus1)?
        .affine(1.0, 1e-12)?
        .sqrt()?;
    xs.broadcast_div(&norm)
}

/// Maps `[0, 1]` RGB pixels `(batch, 3, S, S)` to CLIP's normalised input.
pub fn normalize_pixels(pixels: &Tensor) -> Result<Tensor> {
    let device: &Device = pixels.device();
    let mean = Tensor::from_slice(&CLIP_MEAN, (1, 3, 1, 1), device)?;
    let std = Tensor::from_slice(&CLIP_STD, (1, 3, 1, 1), device)?;
    pixels.broadcast_sub(&mean)?.broadcast_div(&std)
}

/// Deterministic random weights under the real checkpoint's tensor names.
/// For tests and smoke runs only: a model built from these knows nothing.
pub fn random_weights(
    cfg: &ClipVisionConfig,
    seed: u64,
) -> Result<std::collections::HashMap<String, Tensor>> {
    let mut rng = crate::rng::Rng::new(seed);
    let mut map = std::collections::HashMap::new();
    let mut put = |name: String, shape: &[usize], scale: f32, offset: f32| -> Result<()> {
        let n: usize = shape.iter().product();
        let values: Vec<f32> = (0..n).map(|_| offset + scale * rng.gaussian()).collect();
        map.insert(name, Tensor::from_vec(values, shape, &Device::Cpu)?);
        Ok(())
    };
    let d = cfg.embed_dim;
    let i = cfg.intermediate_size;
    let p = cfg.patch_size;
    put(
        "vision_model.embeddings.class_embedding".into(),
        &[d],
        0.1,
        0.0,
    )?;
    put(
        "vision_model.embeddings.patch_embedding.weight".into(),
        &[d, 3, p, p],
        0.1,
        0.0,
    )?;
    put(
        "vision_model.embeddings.position_embedding.weight".into(),
        &[cfg.num_positions(), d],
        0.1,
        0.0,
    )?;
    for ln in ["vision_model.pre_layrnorm", "vision_model.post_layernorm"] {
        put(format!("{ln}.weight"), &[d], 0.1, 1.0)?;
        put(format!("{ln}.bias"), &[d], 0.1, 0.0)?;
    }
    for l in 0..cfg.num_layers {
        let base = format!("vision_model.encoder.layers.{l}");
        for ln in ["layer_norm1", "layer_norm2"] {
            put(format!("{base}.{ln}.weight"), &[d], 0.1, 1.0)?;
            put(format!("{base}.{ln}.bias"), &[d], 0.1, 0.0)?;
        }
        for proj in ["q_proj", "k_proj", "v_proj", "out_proj"] {
            put(format!("{base}.self_attn.{proj}.weight"), &[d, d], 0.1, 0.0)?;
            put(format!("{base}.self_attn.{proj}.bias"), &[d], 0.1, 0.0)?;
        }
        put(format!("{base}.mlp.fc1.weight"), &[i, d], 0.1, 0.0)?;
        put(format!("{base}.mlp.fc1.bias"), &[i], 0.1, 0.0)?;
        put(format!("{base}.mlp.fc2.weight"), &[d, i], 0.1, 0.0)?;
        put(format!("{base}.mlp.fc2.bias"), &[d], 0.1, 0.0)?;
    }
    put(
        "visual_projection.weight".into(),
        &[cfg.projection_dim, d],
        0.1,
        0.0,
    )?;
    Ok(map)
}

#[cfg(test)]
pub(crate) mod testing {
    use super::{random_weights, ClipVisionConfig};
    use candle_core::{DType, Device, Tensor};
    use candle_nn::VarBuilder;
    use std::collections::HashMap;

    pub fn vision_weights(cfg: &ClipVisionConfig, seed: u64) -> HashMap<String, Tensor> {
        random_weights(cfg, seed).unwrap()
    }

    pub fn tiny_model(seed: u64) -> super::ClipVision {
        let cfg = ClipVisionConfig::tiny();
        let vb = VarBuilder::from_tensors(vision_weights(&cfg, seed), DType::F32, &Device::Cpu);
        super::ClipVision::new(vb, &cfg).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{tiny_model, vision_weights};
    use super::*;
    use candle_core::{DType, Var};

    #[test]
    fn forward_has_the_expected_shape() {
        let model = tiny_model(1);
        let x = Tensor::zeros((3, 3, 32, 32), DType::F32, &Device::Cpu).unwrap();
        let y = model.forward(&x).unwrap();
        assert_eq!(y.dims(), &[3, 8]);
        assert!(y
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap()
            .iter()
            .all(|v| v.is_finite()));
    }

    #[test]
    fn gradient_reaches_the_input_pixels() {
        let model = tiny_model(2);
        let values: Vec<f32> = (0..2 * 3 * 32 * 32)
            .map(|i| ((i * 31) % 97) as f32 / 97.0)
            .collect();
        let pixels =
            Var::from_tensor(&Tensor::from_vec(values, (2, 3, 32, 32), &Device::Cpu).unwrap())
                .unwrap();
        let loss = model
            .forward(pixels.as_tensor())
            .unwrap()
            .sqr()
            .unwrap()
            .sum_all()
            .unwrap();
        let grads = loss.backward().unwrap();
        let g = grads
            .get(pixels.as_tensor())
            .expect("no gradient reached the input: the graph is detached");
        let abs_sum: f32 = g.abs().unwrap().sum_all().unwrap().to_scalar().unwrap();
        assert!(abs_sum.is_finite() && abs_sum > 0.0, "abs_sum {abs_sum}");
    }

    #[test]
    fn l2_normalize_gives_unit_rows() {
        let x =
            Tensor::from_vec(vec![3f32, 4.0, 0.0, 1.0, 2.0, 2.0], (2, 3), &Device::Cpu).unwrap();
        let n = l2_normalize(&x).unwrap();
        let sq = n
            .sqr()
            .unwrap()
            .sum(D::Minus1)
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        assert!(sq.iter().all(|v| (v - 1.0).abs() < 1e-5));
    }

    #[test]
    fn normalize_pixels_matches_manual_formula() {
        let x = Tensor::from_vec(vec![0.5f32; 3 * 2 * 2], (1, 3, 2, 2), &Device::Cpu).unwrap();
        let n = normalize_pixels(&x)
            .unwrap()
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        for c in 0..3 {
            let want = (0.5 - CLIP_MEAN[c]) / CLIP_STD[c];
            assert!((n[c * 4] - want).abs() < 1e-5);
        }
    }

    #[test]
    fn rejects_inconsistent_config() {
        let mut cfg = ClipVisionConfig::tiny();
        cfg.num_heads = 3;
        let vb = VarBuilder::from_tensors(
            vision_weights(&ClipVisionConfig::tiny(), 1),
            DType::F32,
            &Device::Cpu,
        );
        assert!(ClipVision::new(vb.clone(), &cfg).is_err());
        let mut cfg = ClipVisionConfig::tiny();
        cfg.patch_size = 5;
        assert!(ClipVision::new(vb, &cfg).is_err());
    }

    #[test]
    fn missing_weights_are_an_error() {
        let vb =
            VarBuilder::from_tensors(std::collections::HashMap::new(), DType::F32, &Device::Cpu);
        assert!(ClipVision::new(vb, &ClipVisionConfig::tiny()).is_err());
    }
}
