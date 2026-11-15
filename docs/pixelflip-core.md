# pixelflip-core

## Overview

Pattern-based pixel negation effects. Standalone visual-effects crate: it
negates pixels wherever a chosen pattern says to, for a stylized look. Not
part of the anti-AI tooling in this workspace, no relationship to
`gindema-*` or `mirage-*` beyond living in the same repo. Mirrors the
`pixelflip.html` browser prototype formula-for-formula (same hash
constants, same rotation math) so the two tools agree on a given image and
set of parameters.

## Modules

### `lib.rs`

**What it does:** Defines `Pattern` (Checker, Stripes, Diagonal, Radial,
Noise, Luminance) and `Params` (pattern + tile size + blend strength), and
`apply()`, which negates pixels in place wherever `flips()` says to for the
chosen pattern.

- `Checker`: alternating square tiles.
- `Stripes { angle }`: repeating bands at an arbitrary angle, via a
  rotated-coordinate projection.
- `Diagonal`: the same rotated-coordinate trick applied twice, at 45 and
  135 degrees, then checkerboarded on both; reads as a diamond grid rather
  than squares or bands. Added because `Stripes` alone, even at a 45 degree
  angle, is still just one set of parallel bands, not the tiled-diamond
  look this is going for.
- `Radial`: concentric rings from the image center.
- `Noise { threshold }`: per-block deterministic pseudo-random threshold,
  via a sine-based hash kept bit-for-bit identical to the one in
  `pixelflip.html`'s JS.
- `Luminance { threshold }`: content-driven rather than geometric, negates
  wherever a pixel's brightness falls below the threshold.

## CI and Workflows

See `.github/workflows/ci.yml` (workspace-wide, `workflow_dispatch` only).

## Fixes and Problems

- The crate itself compiles clean on Rust 1.75 (this workspace's floor),
  but `image`'s and `clap`'s *current* releases pull in transitive
  dependencies that require Cargo's `edition2024` feature, which Rust 1.75
  doesn't support. Confirmed by actually running `cargo check` against
  current `image = "0.25"` and `clap = "4.6"`, both failed the same way.
  Verified working or against pinned, pre-`edition2024` versions instead
  (`image = "=0.24.9"`, `clap = "=4.5.4"`, `anyhow = "=1.0.86"`), which do
  compile and pass tests under 1.75. Whether that pin is the right
  long-term answer, versus raising the workspace's `rust-version`, is an
  open decision, see the root `README.md` note on this.
- Resolved: the workspace `rust-version` was raised instead of pinning old versions, first to 1.85 and then to 1.88 (`image 0.25.10` itself requires 1.88, see the root `README.md`). The `=0.24.9` / `=4.5.4` / `=1.0.86` pins above are no longer used; `Cargo.toml` now uses `image = "0.25"`, `clap = "4.6"`, `anyhow = "1"`. This crate only touches `image` APIs that are unchanged between 0.24 and 0.25 (`open`, `to_rgb8`, `get_pixel`, `put_pixel`, `from_pixel`, `save`).
