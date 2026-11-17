// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-core.md, section "pattern.rs"
// ============================================================================

//! The perturbation pattern: a small low-resolution RGB grid of signed offsets
//! (in `[-epsilon, +epsilon]`, pixel units where 1.0 = full scale), bilinearly
//! stretched over a frame and added to it. Stored low-resolution on purpose:
//! a low-frequency perturbation is what survives H.264/JPEG re-encoding.

use std::fmt;

/// File magic for serialized patterns.
pub const MAGIC: [u8; 4] = *b"MIRP";
/// Current serialization version.
pub const VERSION: u16 = 1;
/// Number of colour channels (always RGB).
pub const CHANNELS: usize = 3;

/// magic (4) + version (2) + channels (2) + width (4) + height (4) + epsilon (4)
const HEADER_LEN: usize = 20;
/// Slack when checking stored values against epsilon (f32 rounding).
const EPSILON_SLACK: f32 = 1e-6;

/// Everything that can go wrong loading, building, or applying a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// The file does not start with the `MIRP` magic.
    BadMagic,
    /// The file was written by a newer/older incompatible version.
    UnsupportedVersion(u16),
    /// Only 3-channel (RGB) patterns exist.
    UnsupportedChannels(u16),
    /// The byte length does not match what the header promises.
    BadLength {
        /// Length the header implies.
        expected: usize,
        /// Length actually supplied.
        actual: usize,
    },
    /// Width or height is zero (or too large to represent).
    BadSize,
    /// Epsilon is not a finite positive number.
    InvalidEpsilon,
    /// A pattern value is NaN or infinite.
    NonFinite,
    /// A pattern value lies outside `[-epsilon, +epsilon]`.
    ExceedsEpsilon,
    /// The frame buffer length is not `width * height * 3`.
    FrameSize {
        /// Length the stated frame dimensions imply.
        expected: usize,
        /// Length actually supplied.
        actual: usize,
    },
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not a mirage pattern file (bad magic)"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported pattern version {v}"),
            Self::UnsupportedChannels(c) => write!(f, "unsupported channel count {c}"),
            Self::BadLength { expected, actual } => {
                write!(
                    f,
                    "pattern length mismatch: expected {expected} bytes, got {actual}"
                )
            }
            Self::BadSize => write!(f, "pattern width/height must be non-zero"),
            Self::InvalidEpsilon => write!(f, "epsilon must be finite and positive"),
            Self::NonFinite => write!(f, "pattern contains a non-finite value"),
            Self::ExceedsEpsilon => write!(f, "pattern value exceeds its epsilon bound"),
            Self::FrameSize { expected, actual } => {
                write!(f, "frame buffer is {actual} bytes, expected {expected}")
            }
        }
    }
}

impl std::error::Error for PatternError {}

/// One output sample of a 1-D linear interpolation: blend source samples
/// `lo` and `hi`, giving `hi` the weight `w_hi` and `lo` the weight `1 - w_hi`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tap {
    /// Lower source index.
    pub lo: usize,
    /// Upper source index (equal to `lo` at the edges).
    pub hi: usize,
    /// Weight of the upper sample, in `[0, 1]`.
    pub w_hi: f32,
}

/// Linear-interpolation taps for resizing `src` samples to `dst` samples.
/// Half-pixel centres, edges clamped (the `align_corners = false` convention).
/// All weights are non-negative and sum to 1, so interpolation can never push
/// a value outside the range of its inputs. `mirage-train` builds its
/// differentiable upsampling matrices from these same taps, so training and
/// runtime stretch the pattern identically.
pub fn interp_taps(src: usize, dst: usize) -> Vec<Tap> {
    interp_taps_window(src, dst, 0.0, 1.0)
}

/// Like [`interp_taps`], but only the window `[start, start + extent]` of the
/// source (fractions of its length, `0.0..=1.0`) is stretched to `dst` samples.
/// This is what an encoder that centre-crops a frame sees of a pattern that
/// was stretched over the whole frame. Returns no taps for a zero size, a
/// non-finite window, or a non-positive extent.
pub fn interp_taps_window(src: usize, dst: usize, start: f64, extent: f64) -> Vec<Tap> {
    if src == 0 || dst == 0 || !start.is_finite() || !extent.is_finite() || extent <= 0.0 {
        return Vec::new();
    }
    let last = (src - 1) as f64;
    let origin = start * src as f64;
    let span = extent * src as f64;
    (0..dst)
        .map(|i| {
            let pos = (origin + (i as f64 + 0.5) * span / dst as f64 - 0.5).clamp(0.0, last);
            let lo = pos.floor() as usize;
            let hi = (lo + 1).min(src - 1);
            Tap {
                lo,
                hi,
                w_hi: (pos - lo as f64) as f32,
            }
        })
        .collect()
}

/// A low-resolution RGB perturbation grid with an L-infinity bound.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    width: usize,
    height: usize,
    epsilon: f32,
    /// Row-major, interleaved RGB: index `(y * width + x) * 3 + channel`.
    data: Vec<f32>,
}

impl Pattern {
    /// Builds a pattern, validating size, epsilon and every value.
    pub fn new(
        width: usize,
        height: usize,
        epsilon: f32,
        data: Vec<f32>,
    ) -> Result<Self, PatternError> {
        if width == 0
            || height == 0
            || u32::try_from(width).is_err()
            || u32::try_from(height).is_err()
        {
            return Err(PatternError::BadSize);
        }
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(PatternError::InvalidEpsilon);
        }
        let expected = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(CHANNELS))
            .ok_or(PatternError::BadSize)?;
        if data.len() != expected {
            return Err(PatternError::BadLength {
                expected,
                actual: data.len(),
            });
        }
        if data.iter().any(|v| !v.is_finite()) {
            return Err(PatternError::NonFinite);
        }
        if data.iter().any(|v| v.abs() > epsilon + EPSILON_SLACK) {
            return Err(PatternError::ExceedsEpsilon);
        }
        Ok(Self {
            width,
            height,
            epsilon,
            data,
        })
    }

    /// Grid width in samples.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Grid height in samples.
    pub fn height(&self) -> usize {
        self.height
    }

    /// L-infinity bound, in pixel units where 1.0 is full scale.
    pub fn epsilon(&self) -> f32 {
        self.epsilon
    }

    /// Raw grid values, row-major interleaved RGB.
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Serializes to the little-endian `MIRP` v1 format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.data.len() * 4);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
        out.extend_from_slice(&(self.width as u32).to_le_bytes());
        out.extend_from_slice(&(self.height as u32).to_le_bytes());
        out.extend_from_slice(&self.epsilon.to_le_bytes());
        for v in &self.data {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    /// Parses the `MIRP` v1 format, re-validating everything.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PatternError> {
        if bytes.len() < HEADER_LEN {
            return Err(PatternError::BadLength {
                expected: HEADER_LEN,
                actual: bytes.len(),
            });
        }
        if bytes[0..4] != MAGIC {
            return Err(PatternError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(PatternError::UnsupportedVersion(version));
        }
        let channels = u16::from_le_bytes([bytes[6], bytes[7]]);
        if usize::from(channels) != CHANNELS {
            return Err(PatternError::UnsupportedChannels(channels));
        }
        let width = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let height = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let epsilon = f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);

        let values = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(CHANNELS))
            .ok_or(PatternError::BadSize)?;
        let expected = values
            .checked_mul(4)
            .and_then(|n| n.checked_add(HEADER_LEN))
            .ok_or(PatternError::BadSize)?;
        if bytes.len() != expected {
            return Err(PatternError::BadLength {
                expected,
                actual: bytes.len(),
            });
        }
        // The length check above guarantees an exact multiple of 4 bytes.
        let (chunks, _) = bytes[HEADER_LEN..].as_chunks::<4>();
        let data = chunks.iter().map(|c| f32::from_le_bytes(*c)).collect();
        Self::new(width, height, epsilon, data)
    }

    /// Bilinear sample of channel `c` at the given taps.
    fn sample(&self, ty: &Tap, tx: &Tap, c: usize) -> f32 {
        let at = |y: usize, x: usize| self.data[(y * self.width + x) * CHANNELS + c];
        let top = at(ty.lo, tx.lo) * (1.0 - tx.w_hi) + at(ty.lo, tx.hi) * tx.w_hi;
        let bottom = at(ty.hi, tx.lo) * (1.0 - tx.w_hi) + at(ty.hi, tx.hi) * tx.w_hi;
        top * (1.0 - ty.w_hi) + bottom * ty.w_hi
    }

    /// Stretches the grid to `dst_w` x `dst_h`; row-major interleaved RGB.
    /// Every output value stays within `[-epsilon, +epsilon]`.
    pub fn upsample(&self, dst_w: usize, dst_h: usize) -> Vec<f32> {
        let xs = interp_taps(self.width, dst_w);
        let ys = interp_taps(self.height, dst_h);
        let mut out = Vec::with_capacity(xs.len() * ys.len() * CHANNELS);
        for ty in &ys {
            for tx in &xs {
                for c in 0..CHANNELS {
                    out.push(self.sample(ty, tx, c));
                }
            }
        }
        out
    }

    /// Adds the stretched pattern to an interleaved RGB8 frame in place,
    /// rounding to the nearest level and clamping to `0..=255`. Per-channel
    /// change is at most `round(epsilon * 255)` levels.
    pub fn apply_rgb8(
        &self,
        frame: &mut [u8],
        width: usize,
        height: usize,
    ) -> Result<(), PatternError> {
        let expected = width.saturating_mul(height).saturating_mul(CHANNELS);
        if frame.len() != expected {
            return Err(PatternError::FrameSize {
                expected,
                actual: frame.len(),
            });
        }
        let xs = interp_taps(self.width, width);
        let ys = interp_taps(self.height, height);
        let mut i = 0;
        for ty in &ys {
            for tx in &xs {
                for c in 0..CHANNELS {
                    let value = f32::from(frame[i]) + self.sample(ty, tx, c) * 255.0;
                    frame[i] = value.round().clamp(0.0, 255.0) as u8;
                    i += 1;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_pattern(w: usize, h: usize, eps: f32) -> Pattern {
        let data = (0..w * h * CHANNELS)
            .map(|i| eps * (((i * 7919) % 201) as f32 / 100.0 - 1.0))
            .collect();
        Pattern::new(w, h, eps, data).unwrap()
    }

    #[test]
    fn taps_are_convex_and_in_range() {
        for (src, dst) in [(4, 4), (4, 17), (28, 224), (7, 3), (1, 9)] {
            let taps = interp_taps(src, dst);
            assert_eq!(taps.len(), dst);
            for t in taps {
                assert!(t.lo < src && t.hi < src && t.lo <= t.hi);
                assert!((0.0..=1.0).contains(&t.w_hi));
            }
        }
        assert!(interp_taps(0, 5).is_empty());
        assert!(interp_taps(5, 0).is_empty());
    }

    #[test]
    fn full_window_equals_plain_taps() {
        for (src, dst) in [(4, 4), (28, 224), (7, 3)] {
            assert_eq!(
                interp_taps(src, dst),
                interp_taps_window(src, dst, 0.0, 1.0)
            );
        }
    }

    #[test]
    fn centred_half_window_samples_the_middle_of_the_source() {
        // Source [2, 6) of 8 samples stretched to 4: positions land exactly on 2, 3, 4, 5.
        let taps = interp_taps_window(8, 4, 0.25, 0.5);
        let los: Vec<usize> = taps.iter().map(|t| t.lo).collect();
        assert_eq!(los, [2, 3, 4, 5]);
        assert!(taps.iter().all(|t| t.w_hi.abs() < 1e-6));
    }

    #[test]
    fn windows_past_the_edge_clamp_and_bad_windows_are_empty() {
        let taps = interp_taps_window(5, 6, 0.9, 0.5);
        assert_eq!(taps.len(), 6);
        assert!(taps
            .iter()
            .all(|t| t.lo < 5 && t.hi < 5 && (0.0..=1.0).contains(&t.w_hi)));
        for (start, extent) in [
            (f64::NAN, 0.5),
            (0.0, f64::INFINITY),
            (0.0, 0.0),
            (0.0, -1.0),
        ] {
            assert!(interp_taps_window(5, 6, start, extent).is_empty());
        }
    }

    #[test]
    fn identity_resize_reproduces_the_grid() {
        let p = gradient_pattern(5, 4, 0.03);
        let up = p.upsample(5, 4);
        for (a, b) in up.iter().zip(p.data()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn upsample_stays_within_epsilon() {
        let p = gradient_pattern(6, 6, 0.03);
        let up = p.upsample(100, 57);
        assert_eq!(up.len(), 100 * 57 * CHANNELS);
        assert!(up.iter().all(|v| v.abs() <= 0.03 + 1e-6));
    }

    #[test]
    fn constant_grid_upsamples_to_constant() {
        let p = Pattern::new(3, 3, 0.05, vec![0.02; 27]).unwrap();
        assert!(p.upsample(11, 7).iter().all(|v| (v - 0.02).abs() < 1e-6));
    }

    #[test]
    fn bytes_roundtrip() {
        let p = gradient_pattern(8, 5, 0.031);
        let back = Pattern::from_bytes(&p.to_bytes()).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn rejects_corrupt_files() {
        let good = gradient_pattern(4, 4, 0.03).to_bytes();

        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        assert_eq!(Pattern::from_bytes(&bad_magic), Err(PatternError::BadMagic));

        let mut bad_version = good.clone();
        bad_version[4] = 9;
        assert_eq!(
            Pattern::from_bytes(&bad_version),
            Err(PatternError::UnsupportedVersion(9))
        );

        let mut bad_channels = good.clone();
        bad_channels[6] = 4;
        assert_eq!(
            Pattern::from_bytes(&bad_channels),
            Err(PatternError::UnsupportedChannels(4))
        );

        assert!(matches!(
            Pattern::from_bytes(&good[..good.len() - 1]),
            Err(PatternError::BadLength { .. })
        ));
        assert!(matches!(
            Pattern::from_bytes(&good[..10]),
            Err(PatternError::BadLength { .. })
        ));

        let mut nan = good.clone();
        nan[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert_eq!(Pattern::from_bytes(&nan), Err(PatternError::NonFinite));

        let mut too_big = good.clone();
        too_big[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&1.0f32.to_le_bytes());
        assert_eq!(
            Pattern::from_bytes(&too_big),
            Err(PatternError::ExceedsEpsilon)
        );
    }

    #[test]
    fn constructor_validates() {
        assert_eq!(Pattern::new(0, 4, 0.03, vec![]), Err(PatternError::BadSize));
        assert_eq!(
            Pattern::new(1, 1, 0.0, vec![0.0; 3]),
            Err(PatternError::InvalidEpsilon)
        );
        assert_eq!(
            Pattern::new(1, 1, f32::NAN, vec![0.0; 3]),
            Err(PatternError::InvalidEpsilon)
        );
        assert!(matches!(
            Pattern::new(2, 2, 0.03, vec![0.0; 5]),
            Err(PatternError::BadLength { .. })
        ));
    }

    #[test]
    fn apply_adds_clamps_and_bounds_the_change() {
        let eps = 8.0 / 255.0;
        let p = Pattern::new(1, 1, eps, vec![eps; 3]).unwrap();

        let mut frame = vec![100u8; 4 * 3 * CHANNELS];
        p.apply_rgb8(&mut frame, 4, 3).unwrap();
        assert!(frame.iter().all(|&v| v == 108));

        let mut bright = vec![252u8; 12];
        p.apply_rgb8(&mut bright, 2, 2).unwrap();
        assert!(bright.iter().all(|&v| v == 255));

        let neg = Pattern::new(1, 1, eps, vec![-eps; 3]).unwrap();
        let mut dark = vec![3u8; 12];
        neg.apply_rgb8(&mut dark, 2, 2).unwrap();
        assert!(dark.iter().all(|&v| v == 0));
    }

    #[test]
    fn apply_never_changes_a_channel_by_more_than_epsilon() {
        let eps = 8.0 / 255.0;
        let p = gradient_pattern(7, 5, eps);
        let original: Vec<u8> = (0..64 * 48 * CHANNELS).map(|i| (i % 251) as u8).collect();
        let mut frame = original.clone();
        p.apply_rgb8(&mut frame, 64, 48).unwrap();
        let limit = (eps * 255.0).round() as i32;
        for (a, b) in original.iter().zip(&frame) {
            assert!((i32::from(*a) - i32::from(*b)).abs() <= limit);
        }
    }

    #[test]
    fn apply_rejects_wrong_buffer_size() {
        let p = gradient_pattern(2, 2, 0.03);
        let mut frame = vec![0u8; 10];
        assert_eq!(
            p.apply_rgb8(&mut frame, 2, 2),
            Err(PatternError::FrameSize {
                expected: 12,
                actual: 10
            })
        );
    }
}
