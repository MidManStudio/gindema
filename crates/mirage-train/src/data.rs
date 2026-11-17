// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "data.rs"
// ============================================================================

//! Image loading and CLIP-style preprocessing: shorter side resized to the
//! model's resolution (Catmull-Rom), then a centre crop. Training and
//! evaluation share this so they see frames the way the encoder does.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use candle_core::{Device, Tensor};
use image::{imageops::FilterType, RgbImage};
use mirage_core::Pattern;

const EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// Lists `png`/`jpg`/`jpeg` files in `dir`, sorted by path so runs are repeatable.
pub fn list_images(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading image directory {}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        })
        .collect();
    paths.sort();
    if paths.is_empty() {
        bail!("no png/jpg/jpeg images found in {}", dir.display());
    }
    Ok(paths)
}

/// Decodes an image file to RGB8.
pub fn load_rgb(path: &Path) -> Result<RgbImage> {
    Ok(image::open(path)
        .with_context(|| format!("decoding {}", path.display()))?
        .to_rgb8())
}

/// Which part of a frame-wide pattern the encoder sees after preprocessing.
/// A pattern is stretched over the *whole* frame at runtime, but CLIP-style
/// preprocessing centre-crops, so for a non-square frame the encoder only sees
/// a window of it. Coordinates are fractions of the original frame, `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropWindow {
    /// Left edge of the visible window.
    pub x0: f64,
    /// Width of the visible window.
    pub x_extent: f64,
    /// Top edge of the visible window.
    pub y0: f64,
    /// Height of the visible window.
    pub y_extent: f64,
}

impl CropWindow {
    /// The whole frame is visible (a square frame, or the old assumption that
    /// the pattern covers exactly the cropped image).
    pub const FULL: Self = Self {
        x0: 0.0,
        x_extent: 1.0,
        y0: 0.0,
        y_extent: 1.0,
    };
}

/// Resize target and crop origin shared by `preprocess` and `crop_window`, so
/// the pixels and the pattern window can never disagree.
fn geometry(w: u32, h: u32, size: u32) -> (u32, u32, u32, u32) {
    let scale = f64::from(size) / f64::from(w.min(h));
    let new_w = ((f64::from(w) * scale).round() as u32).max(size);
    let new_h = ((f64::from(h) * scale).round() as u32).max(size);
    (new_w, new_h, (new_w - size) / 2, (new_h - size) / 2)
}

/// The part of a `w` x `h` frame that `preprocess(.., size)` keeps.
pub fn crop_window(w: u32, h: u32, size: u32) -> CropWindow {
    let (new_w, new_h, x0, y0) = geometry(w, h, size);
    CropWindow {
        x0: f64::from(x0) / f64::from(new_w),
        x_extent: f64::from(size) / f64::from(new_w),
        y0: f64::from(y0) / f64::from(new_h),
        y_extent: f64::from(size) / f64::from(new_h),
    }
}

/// Resizes the shorter side to `size`, centre-crops to `size` x `size`, and
/// returns planar CHW floats in `[0, 1]`.
pub fn preprocess(img: &RgbImage, size: u32) -> Vec<f32> {
    let (w, h) = img.dimensions();
    let (new_w, new_h, x0, y0) = geometry(w, h, size);
    let resized = image::imageops::resize(img, new_w, new_h, FilterType::CatmullRom);
    let cropped = image::imageops::crop_imm(&resized, x0, y0, size, size).to_image();

    let s = size as usize;
    let mut out = vec![0f32; 3 * s * s];
    for (x, y, px) in cropped.enumerate_pixels() {
        for c in 0..3 {
            out[c * s * s + y as usize * s + x as usize] = f32::from(px[c]) / 255.0;
        }
    }
    out
}

/// `preprocess` plus the window of a frame-wide pattern that the crop shows.
pub fn preprocess_with_window(img: &RgbImage, size: u32) -> (Vec<f32>, CropWindow) {
    let (w, h) = img.dimensions();
    (preprocess(img, size), crop_window(w, h, size))
}

/// Stacks preprocessed images into one `(N, 3, S, S)` tensor.
pub fn stack_images(
    images: &[Vec<f32>],
    size: usize,
    device: &Device,
) -> candle_core::Result<Tensor> {
    let mut flat = Vec::with_capacity(images.len() * 3 * size * size);
    for image in images {
        flat.extend_from_slice(image);
    }
    Tensor::from_vec(flat, (images.len(), 3, size, size), device)
}

/// Renders a pattern as a visible PNG-ready image: zero maps to mid-grey and
/// `+-epsilon` to the extremes, stretched to `size` x `size`.
pub fn pattern_preview(pattern: &Pattern, size: u32) -> RgbImage {
    let s = size as usize;
    let values = pattern.upsample(s, s);
    let eps = pattern.epsilon();
    let bytes: Vec<u8> = values
        .iter()
        .map(|v| (127.5 + 127.5 * (v / eps)).round().clamp(0.0, 255.0) as u8)
        .collect();
    RgbImage::from_raw(size, size, bytes).expect("buffer length is size * size * 3")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn preprocess_crops_the_centre_of_a_wide_image() {
        // 40x20: left half red, right half blue. Shorter side 20 -> 8, so the
        // resized image is 16x8 and the 8x8 crop spans x in 4..12 (4 red, 4 blue).
        let img = RgbImage::from_fn(40, 20, |x, _| {
            if x < 20 {
                Rgb([255, 0, 0])
            } else {
                Rgb([0, 0, 255])
            }
        });
        let out = preprocess(&img, 8);
        assert_eq!(out.len(), 3 * 8 * 8);
        let red = &out[0..64];
        let blue = &out[2 * 64..3 * 64];
        assert!(
            red[0] > 0.9 && red[7] < 0.1,
            "left edge red, right edge not"
        );
        assert!(
            blue[0] < 0.1 && blue[7] > 0.9,
            "left edge not blue, right edge blue"
        );
        assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn crop_window_matches_what_preprocess_keeps() {
        assert_eq!(crop_window(8, 8, 8), CropWindow::FULL);
        assert_eq!(crop_window(100, 100, 32), CropWindow::FULL);

        // 40x20 -> 16x8 -> crop x in 4..12: starts a quarter in, half as wide.
        let wide = crop_window(40, 20, 8);
        assert_eq!((wide.x0, wide.x_extent), (0.25, 0.5));
        assert_eq!((wide.y0, wide.y_extent), (0.0, 1.0));

        // 20x40 is the same, transposed.
        let tall = crop_window(20, 40, 8);
        assert_eq!((tall.y0, tall.y_extent), (0.25, 0.5));
        assert_eq!((tall.x0, tall.x_extent), (0.0, 1.0));

        // A typical 640x480 photo at 224 keeps roughly the central 75%
        // horizontally (the integer pixel crop makes it 224 of 299).
        let photo = crop_window(640, 480, 224);
        assert!((photo.x_extent - 0.75).abs() < 5e-3, "{photo:?}");
        assert!((photo.x0 - 0.125).abs() < 5e-3, "{photo:?}");
        assert_eq!((photo.y0, photo.y_extent), (0.0, 1.0));

        let img = RgbImage::from_pixel(40, 20, Rgb([1, 2, 3]));
        let (pixels, window) = preprocess_with_window(&img, 8);
        assert_eq!(pixels.len(), 3 * 64);
        assert_eq!(window, wide);
    }

    #[test]
    fn preprocess_handles_tall_and_square_images() {
        let tall = RgbImage::from_pixel(10, 30, Rgb([10, 20, 30]));
        assert_eq!(preprocess(&tall, 8).len(), 192);
        let square = RgbImage::from_pixel(8, 8, Rgb([10, 20, 30]));
        let out = preprocess(&square, 8);
        assert!((out[0] - 10.0 / 255.0).abs() < 1e-5);
        assert!((out[64] - 20.0 / 255.0).abs() < 1e-5);
    }

    #[test]
    fn stack_images_builds_nchw() {
        let a = vec![0.0f32; 3 * 4 * 4];
        let b = vec![1.0f32; 3 * 4 * 4];
        let t = stack_images(&[a, b], 4, &Device::Cpu).unwrap();
        assert_eq!(t.dims(), &[2, 3, 4, 4]);
    }

    #[test]
    fn list_images_filters_and_sorts() {
        let dir = std::env::temp_dir().join(format!("mirage-train-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.png", "a.JPG", "c.txt", "d.jpeg"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let names: Vec<String> = list_images(&dir)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.JPG", "b.png", "d.jpeg"]);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(list_images(&dir).is_err());
    }

    #[test]
    fn preview_maps_zero_to_grey_and_extremes_to_black_and_white() {
        let eps = 0.03;
        let zero = Pattern::new(2, 2, eps, vec![0.0; 12]).unwrap();
        assert!(pattern_preview(&zero, 4)
            .as_raw()
            .iter()
            .all(|&v| v == 128 || v == 127));
        let high = Pattern::new(1, 1, eps, vec![eps; 3]).unwrap();
        assert!(pattern_preview(&high, 4).as_raw().iter().all(|&v| v == 255));
        let low = Pattern::new(1, 1, eps, vec![-eps; 3]).unwrap();
        assert!(pattern_preview(&low, 4).as_raw().iter().all(|&v| v == 0));
    }
}
