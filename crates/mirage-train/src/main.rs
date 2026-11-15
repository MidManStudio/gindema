// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "main.rs"
// ============================================================================

//! Offline training step that produces the adversarial perturbation pattern
//! mirage-core ships with. Runs a PGD-style optimization loop against an
//! open CLIP-family vision encoder (candle-transformers'
//! `models::clip::vision_model::ClipVisionTransformer`) using candle-core's
//! own autodiff (`Var` + `backward()`), no Python ML stack involved. The
//! perturbation is constrained to an L-infinity imperceptibility budget and
//! generated at low resolution then upsampled, so it survives H.264/JPEG
//! re-encoding on platforms like YouTube/TikTok instead of being erased by
//! it. Not part of the shipped runtime: this binary is run once (or
//! whenever the pattern needs regenerating), offline, and its output is
//! what mirage-core embeds and applies per-frame.

fn main() {
    // TODO: scaffold stage only, not yet implemented.
    println!("mirage-train: scaffold stub");
}
