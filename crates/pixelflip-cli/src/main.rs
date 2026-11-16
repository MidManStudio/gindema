// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/pixelflip-cli.md, section "main.rs"
// ============================================================================

//! The pixelflip CLI. Applies a pixelflip-core pattern to an image file.
//! Standalone visual-effects tool, not part of the anti-AI tooling.

use clap::{Parser, ValueEnum};
use pixelflip_core::{apply, Params, Pattern};

#[derive(Copy, Clone, ValueEnum)]
enum PatternArg {
    Checker,
    Stripes,
    Diagonal,
    Radial,
    Noise,
    Luminance,
}

/// Negate pixels where a pattern says to.
#[derive(Parser)]
#[command(name = "pixelflip", version)]
struct Cli {
    /// Input image path.
    input: std::path::PathBuf,

    /// Output image path.
    output: std::path::PathBuf,

    /// Which pattern decides what gets negated.
    #[arg(long, value_enum, default_value = "checker")]
    pattern: PatternArg,

    /// Tile size in pixels (checker/stripes/diagonal/radial/noise).
    #[arg(long, default_value_t = 24.0)]
    size: f32,

    /// Stripe angle in degrees (stripes pattern only).
    #[arg(long, default_value_t = 0.0)]
    angle: f32,

    /// Threshold 0-100 (noise/luminance patterns only).
    #[arg(long, default_value_t = 50.0)]
    threshold: f32,

    /// Invert strength 0-100. 0 leaves the image untouched, 100 is a full negate.
    #[arg(long, default_value_t = 100.0)]
    blend: f32,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let pattern = match cli.pattern {
        PatternArg::Checker => Pattern::Checker,
        PatternArg::Stripes => Pattern::Stripes { angle: cli.angle },
        PatternArg::Diagonal => Pattern::Diagonal,
        PatternArg::Radial => Pattern::Radial,
        PatternArg::Noise => Pattern::Noise {
            threshold: cli.threshold / 100.0,
        },
        PatternArg::Luminance => Pattern::Luminance {
            threshold: cli.threshold / 100.0,
        },
    };

    let mut img = image::open(&cli.input)?.to_rgb8();
    apply(
        &mut img,
        &Params {
            pattern,
            size: cli.size,
            blend: cli.blend / 100.0,
        },
    )?;
    img.save(&cli.output)?;

    println!("wrote {}", cli.output.display());
    Ok(())
}
