// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "eval.rs"
// ============================================================================

//! Evaluation of a pattern the way it would actually be deployed: applied to
//! the full-resolution frame with `mirage-core`, optionally pushed through a
//! JPEG round trip, then preprocessed and embedded exactly like training
//! images. Every number is a mean cosine similarity between the embedding of
//! the clean frame and the embedding of the processed frame, so lower means a
//! stronger effect and 1.0 means the encoder did not notice anything.
//!
//! Two controls keep the numbers honest: a random pattern with the same
//! resolution and budget (does optimisation beat noise?) and JPEG alone with
//! no pattern (how much does compression move the embedding by itself?).

use std::path::PathBuf;

use anyhow::Result;
use candle_core::{Device, Tensor};
use image::RgbImage;
use mirage_core::Pattern;

use crate::clip::{l2_normalize, normalize_pixels, ClipVision};
use crate::data;
use crate::rng::Rng;

/// Mean cosine similarities over the evaluated images.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalReport {
    /// Number of images evaluated.
    pub images: usize,
    /// JPEG quality used for the round-trip columns.
    pub jpeg_quality: u8,
    /// Clean frame vs the same frame after JPEG only (compression baseline).
    pub clean_jpeg_cos: f32,
    /// Clean frame vs frame with the trained pattern.
    pub pattern_cos: f32,
    /// Clean frame vs frame with the trained pattern, after JPEG.
    pub pattern_jpeg_cos: f32,
    /// Clean frame vs frame with a random pattern of equal budget.
    pub random_cos: f32,
    /// Clean frame vs frame with a random pattern of equal budget, after JPEG.
    pub random_jpeg_cos: f32,
}

impl EvalReport {
    /// One-line JSON.
    pub fn to_json(&self) -> String {
        format!(
            "{{\"images\":{},\"jpeg_quality\":{},\"clean_jpeg_cos\":{:.6},\"pattern_cos\":{:.6},\
             \"pattern_jpeg_cos\":{:.6},\"random_cos\":{:.6},\"random_jpeg_cos\":{:.6}}}",
            self.images,
            self.jpeg_quality,
            self.clean_jpeg_cos,
            self.pattern_cos,
            self.pattern_jpeg_cos,
            self.random_cos,
            self.random_jpeg_cos
        )
    }

    /// GitHub-flavoured Markdown table.
    pub fn to_markdown(&self) -> String {
        format!(
            "| Frame vs clean frame | Mean cosine (lower = stronger) |\n|---|---|\n\
             | JPEG q{q} only (baseline) | {:.4} |\n\
             | trained pattern | {:.4} |\n\
             | trained pattern + JPEG q{q} | {:.4} |\n\
             | random pattern, same budget | {:.4} |\n\
             | random pattern + JPEG q{q} | {:.4} |\n\n\
             {} held-out images.{}\n",
            self.clean_jpeg_cos,
            self.pattern_cos,
            self.pattern_jpeg_cos,
            self.random_cos,
            self.random_jpeg_cos,
            self.images,
            if self.images < 20 {
                " Fewer than 20 images: treat these numbers as anecdotal, not a benchmark."
            } else {
                ""
            },
            q = self.jpeg_quality
        )
    }
}

/// Encodes to JPEG at `quality` and decodes again.
pub fn jpeg_roundtrip(img: &RgbImage, quality: u8) -> Result<RgbImage> {
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder.encode(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)?.to_rgb8())
}

/// A pattern of random `+-epsilon` cells with the given grid size.
pub fn random_pattern(width: usize, height: usize, epsilon: f32, seed: u64) -> Result<Pattern> {
    let mut rng = Rng::new(seed);
    let data = (0..width * height * mirage_core::CHANNELS)
        .map(|_| {
            if rng.next_u64() & 1 == 0 {
                epsilon
            } else {
                -epsilon
            }
        })
        .collect();
    Ok(Pattern::new(width, height, epsilon, data)?)
}

fn with_pattern(img: &RgbImage, pattern: &Pattern) -> Result<RgbImage> {
    let mut copy = img.clone();
    let (w, h) = copy.dimensions();
    let buf: &mut [u8] = &mut copy;
    pattern.apply_rgb8(buf, w as usize, h as usize)?;
    Ok(copy)
}

fn embed(model: &ClipVision, img: &RgbImage, device: &Device) -> Result<Vec<f32>> {
    let size = model.image_size();
    let pixels = data::preprocess(img, size as u32);
    let tensor = Tensor::from_vec(pixels, (1, 3, size, size), device)?;
    let features = l2_normalize(&model.forward(&normalize_pixels(&tensor)?)?)?;
    Ok(features.flatten_all()?.to_vec1::<f32>()?)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Evaluates `pattern` on in-memory images.
pub fn evaluate_images(
    model: &ClipVision,
    device: &Device,
    images: &[RgbImage],
    pattern: &Pattern,
    jpeg_quality: u8,
    seed: u64,
) -> Result<EvalReport> {
    let random = random_pattern(pattern.width(), pattern.height(), pattern.epsilon(), seed)?;
    let mut sums = [0f32; 5];
    for img in images {
        let clean = embed(model, img, device)?;
        let patterned = with_pattern(img, pattern)?;
        let randomised = with_pattern(img, &random)?;
        let variants = [
            jpeg_roundtrip(img, jpeg_quality)?,
            patterned.clone(),
            jpeg_roundtrip(&patterned, jpeg_quality)?,
            randomised.clone(),
            jpeg_roundtrip(&randomised, jpeg_quality)?,
        ];
        for (sum, variant) in sums.iter_mut().zip(&variants) {
            *sum += cosine(&clean, &embed(model, variant, device)?);
        }
    }
    let n = images.len().max(1) as f32;
    Ok(EvalReport {
        images: images.len(),
        jpeg_quality,
        clean_jpeg_cos: sums[0] / n,
        pattern_cos: sums[1] / n,
        pattern_jpeg_cos: sums[2] / n,
        random_cos: sums[3] / n,
        random_jpeg_cos: sums[4] / n,
    })
}

/// Loads `paths` and evaluates `pattern` on them.
pub fn evaluate(
    model: &ClipVision,
    device: &Device,
    paths: &[PathBuf],
    pattern: &Pattern,
    jpeg_quality: u8,
    seed: u64,
) -> Result<EvalReport> {
    let images = paths
        .iter()
        .map(|p| data::load_rgb(p))
        .collect::<Result<Vec<_>>>()?;
    evaluate_images(model, device, &images, pattern, jpeg_quality, seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::testing::tiny_model;
    use image::Rgb;

    fn test_image(w: u32, h: u32, seed: u64) -> RgbImage {
        let mut rng = Rng::new(seed);
        RgbImage::from_fn(w, h, |x, y| {
            let base = ((x * 3 + y * 2) % 200) as u8;
            let jitter = (rng.next_f32() * 20.0) as u8;
            Rgb([base.saturating_add(jitter), base, 255 - base])
        })
    }

    #[test]
    fn jpeg_roundtrip_keeps_dimensions_and_is_close() {
        let img = test_image(48, 40, 1);
        let back = jpeg_roundtrip(&img, 90).unwrap();
        assert_eq!(back.dimensions(), (48, 40));
        let diff: f32 = img
            .as_raw()
            .iter()
            .zip(back.as_raw())
            .map(|(a, b)| (f32::from(*a) - f32::from(*b)).abs())
            .sum::<f32>()
            / img.as_raw().len() as f32;
        assert!(diff < 25.0, "mean abs diff {diff}");
    }

    #[test]
    fn random_pattern_is_exactly_plus_minus_epsilon() {
        let p = random_pattern(6, 5, 0.03, 4).unwrap();
        assert!(p.data().iter().all(|v| v.abs() == 0.03));
        assert!(p.data().iter().any(|v| *v > 0.0) && p.data().iter().any(|v| *v < 0.0));
        assert_eq!(p, random_pattern(6, 5, 0.03, 4).unwrap());
    }

    #[test]
    fn report_numbers_are_valid_cosines() {
        let model = tiny_model(31);
        let images = vec![
            test_image(48, 40, 1),
            test_image(40, 48, 2),
            test_image(36, 36, 3),
        ];
        let pattern = random_pattern(8, 8, 8.0 / 255.0, 77).unwrap();
        let report = evaluate_images(&model, &Device::Cpu, &images, &pattern, 75, 5).unwrap();
        assert_eq!(report.images, 3);
        for v in [
            report.clean_jpeg_cos,
            report.pattern_cos,
            report.pattern_jpeg_cos,
            report.random_cos,
            report.random_jpeg_cos,
        ] {
            assert!(
                v.is_finite() && (-1.0001..=1.0001).contains(&v),
                "cosine {v}"
            );
        }
        let json = report.to_json();
        assert!(json.starts_with("{\"images\":3,") && json.ends_with('}'));
        assert!(report.to_markdown().contains("trained pattern + JPEG q75"));
    }

    #[test]
    fn an_unperturbed_frame_has_cosine_one() {
        let model = tiny_model(31);
        let img = test_image(40, 40, 9);
        let a = embed(&model, &img, &Device::Cpu).unwrap();
        let b = embed(&model, &img, &Device::Cpu).unwrap();
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-5);
    }
}
