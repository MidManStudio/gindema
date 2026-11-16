// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/pixelflip-core.md, section "lib.rs"
// ============================================================================

//! Pattern-based pixel negation effects. Not part of the anti-AI tooling,
//! this is a standalone visual-effects crate: it negates pixels wherever a
//! chosen pattern says to, for a stylized look rather than any protective
//! purpose. Mirrors the pixelflip.html prototype formula-for-formula so the
//! browser tool and this crate always agree on a given image.

use image::{ImageResult, Rgb, RgbImage};

/// Which tiling or threshold rule decides whether a pixel gets negated.
#[derive(Debug, Clone, Copy)]
pub enum Pattern {
    /// Square tiles, alternating.
    Checker,
    /// Repeating bands at `angle` degrees.
    Stripes { angle: f32 },
    /// Like `Checker`, but the grid itself is rotated 45 degrees, so the
    /// tiles read as diamonds instead of squares.
    Diagonal,
    /// Concentric rings around the image center.
    Radial,
    /// Per-block pseudo-random noise, `threshold` in 0.0..=1.0.
    Noise { threshold: f32 },
    /// Negate wherever a pixel's luminance falls below `threshold` (0.0..=1.0
    /// of full brightness). Content-driven rather than geometric.
    Luminance { threshold: f32 },
}

/// How strongly to apply the negation where the pattern fires: 0.0 leaves
/// the pixel untouched, 1.0 is a full negate, values in between blend.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub pattern: Pattern,
    /// Tile size in pixels for Checker/Stripes/Diagonal/Radial/Noise.
    pub size: f32,
    pub blend: f32,
}

// Same constants as the hash() in pixelflip.html's JS, kept identical on
// purpose: the browser tool and this crate should agree on Noise output
// for the same inputs. (43_758.547 is the same f32 as the JS literal
// 43758.5453; clippy asks for the shorter spelling.)
fn hash(x: i64, y: i64) -> f32 {
    let v = ((x as f32) * 12.9898 + (y as f32) * 78.233).sin() * 43_758.547;
    v - v.floor()
}

fn rotate(x: f32, y: f32, angle_deg: f32) -> f32 {
    let rad = angle_deg.to_radians();
    x * rad.cos() + y * rad.sin()
}

fn flips(x: u32, y: u32, w: u32, h: u32, p: &Params, rgb: [u8; 3]) -> bool {
    let [r, g, b] = rgb;
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let size = p.size.max(1.0);
    match p.pattern {
        Pattern::Checker => {
            (((x / size).floor() as i64) + ((y / size).floor() as i64)).rem_euclid(2) == 0
        }
        Pattern::Stripes { angle } => {
            let rx = rotate(x, y, angle);
            ((rx / size).floor() as i64).rem_euclid(2) == 0
        }
        Pattern::Diagonal => {
            let rx = rotate(x, y, 45.0);
            let ry = rotate(x, y, 135.0);
            (((rx / size).floor() as i64) + ((ry / size).floor() as i64)).rem_euclid(2) == 0
        }
        Pattern::Radial => {
            let (dx, dy) = (x - w / 2.0, y - h / 2.0);
            let dist = (dx * dx + dy * dy).sqrt();
            ((dist / size).floor() as i64).rem_euclid(2) == 0
        }
        Pattern::Noise { threshold } => {
            let (bx, by) = ((x / size).floor() as i64, (y / size).floor() as i64);
            hash(bx, by) < threshold
        }
        Pattern::Luminance { threshold } => {
            let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
            lum < threshold * 255.0
        }
    }
}

fn negate_channel(v: u8, blend: f32) -> u8 {
    let v = v as f32;
    (v + (255.0 - v - v) * blend).round().clamp(0.0, 255.0) as u8
}

/// Applies the pattern in place. Image dimensions are unchanged.
pub fn apply(img: &mut RgbImage, params: &Params) -> ImageResult<()> {
    let (w, h) = img.dimensions();
    let blend = params.blend.clamp(0.0, 1.0);
    for y in 0..h {
        for x in 0..w {
            let Rgb([r, g, b]) = *img.get_pixel(x, y);
            if flips(x, y, w, h, params, [r, g, b]) {
                img.put_pixel(
                    x,
                    y,
                    Rgb([
                        negate_channel(r, blend),
                        negate_channel(g, blend),
                        negate_channel(b, blend),
                    ]),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checker_negates_roughly_half() {
        let mut img = RgbImage::from_pixel(64, 64, Rgb([10, 20, 30]));
        let params = Params {
            pattern: Pattern::Checker,
            size: 8.0,
            blend: 1.0,
        };
        apply(&mut img, &params).unwrap();
        let negated = img.pixels().filter(|p| p.0 != [10, 20, 30]).count();
        assert!(
            negated > 1500 && negated < 2600,
            "got {negated} negated pixels"
        );
    }

    #[test]
    fn zero_blend_is_a_no_op() {
        let mut img = RgbImage::from_pixel(32, 32, Rgb([5, 5, 5]));
        let before = img.clone();
        let params = Params {
            pattern: Pattern::Radial,
            size: 4.0,
            blend: 0.0,
        };
        apply(&mut img, &params).unwrap();
        assert_eq!(img, before);
    }
}
