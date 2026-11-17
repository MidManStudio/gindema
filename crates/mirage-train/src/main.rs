// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "main.rs"
// ============================================================================

//! `mirage-train`: trains a perturbation pattern against the CLIP image
//! encoder and evaluates it. Run offline; its output file is what
//! `mirage-core` applies to frames.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use candle_core::{DType, Device};
use candle_nn::VarBuilder;
use clap::{Args, Parser, Subcommand};
use mirage_core::Pattern;
use mirage_train::clip::{ClipVision, ClipVisionConfig};
use mirage_train::data::CropWindow;
use mirage_train::{data, eval, pgd};

#[derive(Parser)]
#[command(
    name = "mirage-train",
    version,
    about = "Trains and evaluates the mirage perturbation pattern against CLIP"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Train a pattern on a directory of images, optionally evaluating on held-out ones.
    Train(TrainArgs),
    /// Evaluate an existing pattern file on a directory of images.
    Eval(EvalArgs),
}

#[derive(Args)]
struct TrainArgs {
    /// CLIP `model.safetensors` (openai/clip-vit-base-patch32).
    #[arg(long)]
    weights: PathBuf,
    /// Directory of png/jpg/jpeg training images.
    #[arg(long)]
    images: PathBuf,
    /// Where to write the trained pattern (MIRP format).
    #[arg(long)]
    out: PathBuf,
    /// Also write a visible PNG preview of the pattern.
    #[arg(long)]
    preview: Option<PathBuf>,
    /// Write a Markdown run summary (for CI job summaries).
    #[arg(long)]
    summary_md: Option<PathBuf>,
    /// L-infinity budget in 8-bit levels (8 means 8/255).
    #[arg(long, default_value_t = 8.0)]
    epsilon: f32,
    /// Gradient steps.
    #[arg(long, default_value_t = 200)]
    steps: usize,
    /// Images per step.
    #[arg(long, default_value_t = 4)]
    batch_size: usize,
    /// Initial step size in 8-bit levels (default: epsilon / 16).
    #[arg(long)]
    step_size: Option<f32>,
    /// Side of the trained grid.
    #[arg(long, default_value_t = 28)]
    low_res: usize,
    /// Gaussian noise during training, in 8-bit levels.
    #[arg(long, default_value_t = 2.5)]
    noise_std: f32,
    /// Random scale/shift of each image's pattern window while training, as a
    /// fraction of the window (0 disables).
    #[arg(long, default_value_t = 0.1)]
    jitter: f32,
    /// Treat every training crop as if the pattern covered exactly the crop
    /// (how training worked before aspect-aware windows); for A/B comparison.
    #[arg(long)]
    legacy_full_window: bool,
    /// Seed for everything random.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Number of images (the last N by file name) kept out of training and
    /// used to evaluate the result.
    #[arg(long, default_value_t = 0)]
    holdout: usize,
    /// Also evaluate on this many training images in the same deployed
    /// pipeline, to tell overfitting apart from a pipeline mismatch.
    #[arg(long, default_value_t = 8)]
    eval_train: usize,
    /// JPEG quality for the evaluation round trip.
    #[arg(long, default_value_t = 75)]
    jpeg_quality: u8,
    /// Print progress every N steps.
    #[arg(long, default_value_t = 10)]
    log_every: usize,
    /// Use the tiny test architecture instead of ViT-B/32 (smoke tests only).
    #[arg(long, hide = true)]
    tiny: bool,
}

#[derive(Args)]
struct EvalArgs {
    /// CLIP `model.safetensors` (openai/clip-vit-base-patch32).
    #[arg(long)]
    weights: PathBuf,
    /// Directory of png/jpg/jpeg images to evaluate on.
    #[arg(long)]
    images: PathBuf,
    /// Pattern file to evaluate.
    #[arg(long)]
    pattern: PathBuf,
    /// JPEG quality for the round trip.
    #[arg(long, default_value_t = 75)]
    jpeg_quality: u8,
    /// Seed for the random control pattern.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Use the tiny test architecture instead of ViT-B/32 (smoke tests only).
    #[arg(long, hide = true)]
    tiny: bool,
}

fn load_model(weights: &Path, tiny: bool) -> Result<ClipVision> {
    let bytes =
        std::fs::read(weights).with_context(|| format!("reading weights {}", weights.display()))?;
    let vb = VarBuilder::from_buffered_safetensors(bytes, DType::F32, &Device::Cpu)?;
    let cfg = if tiny {
        ClipVisionConfig::tiny()
    } else {
        ClipVisionConfig::vit_base_patch32()
    };
    Ok(ClipVision::new(vb, &cfg)?)
}

fn json_or_null(report: &Option<eval::EvalReport>) -> String {
    report
        .as_ref()
        .map_or_else(|| "null".to_string(), eval::EvalReport::to_json)
}

fn run_train(args: TrainArgs) -> Result<()> {
    let all = data::list_images(&args.images)?;
    if args.holdout >= all.len() {
        bail!(
            "--holdout {} leaves no training images ({} found)",
            args.holdout,
            all.len()
        );
    }
    let (train_paths, held_paths) = all.split_at(all.len() - args.holdout);
    eprintln!(
        "{} training images, {} held out",
        train_paths.len(),
        held_paths.len()
    );

    let device = Device::Cpu;
    let model = load_model(&args.weights, args.tiny)?;
    let size = model.image_size();
    let mut planar = Vec::with_capacity(train_paths.len());
    let mut windows = Vec::with_capacity(train_paths.len());
    for path in train_paths {
        let (pixels, window) = data::preprocess_with_window(&data::load_rgb(path)?, size as u32);
        planar.push(pixels);
        windows.push(if args.legacy_full_window {
            CropWindow::FULL
        } else {
            window
        });
    }
    let images = data::stack_images(&planar, size, &device)?;

    let levels = 1.0 / 255.0;
    let epsilon = args.epsilon * levels;
    let cfg = pgd::TrainConfig {
        epsilon,
        steps: args.steps,
        batch_size: args.batch_size,
        step_size: args.step_size.map_or(epsilon / 16.0, |s| s * levels),
        low_res: args.low_res,
        noise_std: args.noise_std * levels,
        jitter: args.jitter,
        seed: args.seed,
    };
    let log_every = args.log_every.max(1);
    let outcome = pgd::train(&model, &images, &windows, &cfg, |log| {
        if log.step % log_every == 0 || log.step + 1 == cfg.steps {
            eprintln!(
                "step {:>5}/{}  loss {:.4}  mean cos {:.4}  step size {:.5}",
                log.step + 1,
                cfg.steps,
                log.loss,
                log.mean_cos,
                log.step_size
            );
        }
    })?;
    eprintln!(
        "training-set mean cosine: {:.4} (random start) -> {:.4} (trained)",
        outcome.initial_mean_cos, outcome.final_mean_cos
    );

    std::fs::write(&args.out, outcome.pattern.to_bytes())
        .with_context(|| format!("writing {}", args.out.display()))?;
    eprintln!("wrote pattern to {}", args.out.display());
    if let Some(preview) = &args.preview {
        data::pattern_preview(&outcome.pattern, 256)
            .save(preview)
            .with_context(|| format!("writing {}", preview.display()))?;
        eprintln!("wrote preview to {}", preview.display());
    }

    let evaluate = |paths: &[PathBuf]| -> Result<Option<eval::EvalReport>> {
        if paths.is_empty() {
            return Ok(None);
        }
        Ok(Some(eval::evaluate(
            &model,
            &device,
            paths,
            &outcome.pattern,
            args.jpeg_quality,
            args.seed,
        )?))
    };
    let held_report = evaluate(held_paths)?;
    let train_report = evaluate(&train_paths[..args.eval_train.min(train_paths.len())])?;
    println!(
        "{{\"train\":{},\"heldout\":{}}}",
        json_or_null(&train_report),
        json_or_null(&held_report)
    );

    if let Some(path) = &args.summary_md {
        let window_mode = if args.legacy_full_window {
            "legacy (pattern treated as covering exactly the crop)".to_string()
        } else {
            format!("aspect-aware crop windows, jitter {}", args.jitter)
        };
        let mut md = format!(
            "- **Training images:** {}\n- **Budget:** {} / 255, grid {}x{}, {} steps, batch {}\n\
             - **Pattern window:** {}\n\
             - **Training-set mean cosine (crop pipeline, no JPEG):** {:.4} (random start) -> {:.4} (trained)\n\n",
            train_paths.len(),
            args.epsilon,
            args.low_res,
            args.low_res,
            args.steps,
            args.batch_size,
            window_mode,
            outcome.initial_mean_cos,
            outcome.final_mean_cos
        );
        md.push_str("Both tables below use the deployed pipeline: pattern applied to the full-resolution frame, then (optionally) JPEG, then CLIP preprocessing.\n\n");
        match &held_report {
            Some(report) => {
                md.push_str("#### Held-out images\n\n");
                md.push_str(&report.to_markdown());
            }
            None => md.push_str("_No held-out images, so no held-out evaluation was run._\n"),
        }
        if let Some(report) = &train_report {
            md.push_str("\n#### Training images (in-sample)\n\n");
            md.push_str(&report.to_markdown());
        }
        std::fs::write(path, md).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn run_eval(args: EvalArgs) -> Result<()> {
    let bytes = std::fs::read(&args.pattern)
        .with_context(|| format!("reading {}", args.pattern.display()))?;
    let pattern = Pattern::from_bytes(&bytes)?;
    let paths = data::list_images(&args.images)?;
    let model = load_model(&args.weights, args.tiny)?;
    let report = eval::evaluate(
        &model,
        &Device::Cpu,
        &paths,
        &pattern,
        args.jpeg_quality,
        args.seed,
    )?;
    println!("{}", report.to_json());
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Train(args) => run_train(args),
        Command::Eval(args) => run_eval(args),
    }
}
