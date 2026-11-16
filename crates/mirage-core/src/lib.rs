// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-core.md, section "lib.rs"
// ============================================================================

//! Runtime side of the video/image poisoning tooling. Loads the perturbation
//! pattern that `mirage-train` produced offline and applies it to frames with
//! plain CPU arithmetic: no ML framework, no model weights, no GPU.

pub mod pattern;

pub use pattern::{interp_taps, Pattern, PatternError, Tap, CHANNELS, MAGIC, VERSION};
