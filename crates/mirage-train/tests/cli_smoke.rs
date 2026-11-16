//! End-to-end checks of the `mirage-train` binary on a tiny random model:
//! file handling, flags, output formats and error reporting. These do not
//! (and cannot) say anything about how well a pattern works on real CLIP.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use candle_core::safetensors;
use image::{Rgb, RgbImage};
use mirage_core::Pattern;
use mirage_train::clip::{random_weights, ClipVisionConfig};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mirage-train-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `n` small synthetic PNGs plus tiny random weights; returns (images dir, weights).
fn fixture(dir: &Path, n: u32) -> (PathBuf, PathBuf) {
    let images = dir.join("images");
    std::fs::create_dir_all(&images).unwrap();
    for i in 0..n {
        RgbImage::from_fn(48, 40, |x, y| {
            Rgb([
                ((x * 5 + i * 40) % 256) as u8,
                ((y * 6 + i * 20) % 256) as u8,
                ((x + y + i * 50) % 256) as u8,
            ])
        })
        .save(images.join(format!("img{i}.png")))
        .unwrap();
    }
    let weights = dir.join("tiny.safetensors");
    safetensors::save(
        &random_weights(&ClipVisionConfig::tiny(), 1).unwrap(),
        &weights,
    )
    .unwrap();
    (images, weights)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mirage-train"))
        .args(args)
        .output()
        .unwrap()
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn train_then_eval_end_to_end() {
    let dir = scratch("e2e");
    let (images, weights) = fixture(&dir, 5);
    let pattern_path = dir.join("pattern.bin");
    let preview = dir.join("preview.png");
    let summary = dir.join("summary.md");

    let out = run(&[
        "train",
        "--tiny",
        "--weights",
        s(&weights),
        "--images",
        s(&images),
        "--out",
        s(&pattern_path),
        "--preview",
        s(&preview),
        "--summary-md",
        s(&summary),
        "--epsilon",
        "8",
        "--steps",
        "4",
        "--batch-size",
        "2",
        "--low-res",
        "4",
        "--holdout",
        "2",
        "--log-every",
        "1",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "train failed: {stderr}");
    assert!(stderr.contains("3 training images, 2 held out"), "{stderr}");
    assert!(stderr.contains("step     4/4"), "{stderr}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.trim().starts_with("{\"images\":2,"), "{stdout}");

    let pattern = Pattern::from_bytes(&std::fs::read(&pattern_path).unwrap()).unwrap();
    assert_eq!((pattern.width(), pattern.height()), (4, 4));
    assert!((pattern.epsilon() - 8.0 / 255.0).abs() < 1e-6);
    assert!(image::open(&preview).unwrap().width() == 256);
    let md = std::fs::read_to_string(&summary).unwrap();
    assert!(md.contains("trained pattern") && md.contains("3"), "{md}");

    let out = run(&[
        "eval",
        "--tiny",
        "--weights",
        s(&weights),
        "--images",
        s(&images),
        "--pattern",
        s(&pattern_path),
    ]);
    assert!(
        out.status.success(),
        "eval failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .contains("\"images\":5"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_holdout_that_leaves_nothing_to_train_on_is_rejected() {
    let dir = scratch("holdout");
    let (images, weights) = fixture(&dir, 3);
    let out = run(&[
        "train",
        "--tiny",
        "--weights",
        s(&weights),
        "--images",
        s(&images),
        "--out",
        s(&dir.join("p.bin")),
        "--holdout",
        "3",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("leaves no training images"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn missing_weights_and_bad_patterns_are_reported() {
    let dir = scratch("errors");
    let (images, weights) = fixture(&dir, 3);

    let out = run(&[
        "train",
        "--tiny",
        "--weights",
        s(&dir.join("nope.safetensors")),
        "--images",
        s(&images),
        "--out",
        s(&dir.join("p.bin")),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("reading weights"));

    let garbage = dir.join("garbage.bin");
    std::fs::write(&garbage, b"definitely not a pattern").unwrap();
    let out = run(&[
        "eval",
        "--tiny",
        "--weights",
        s(&weights),
        "--images",
        s(&images),
        "--pattern",
        s(&garbage),
    ]);
    assert!(!out.status.success());
    std::fs::remove_dir_all(&dir).unwrap();
}
