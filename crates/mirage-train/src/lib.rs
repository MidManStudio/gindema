// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/mirage-train.md, section "lib.rs"
// ============================================================================

//! Offline training of the mirage perturbation pattern. Finds one
//! low-resolution RGB pattern that, added to gameplay frames within an
//! L-infinity budget, pushes a CLIP image embedding away from where the clean
//! frame would land. The result is written in `mirage-core`'s `MIRP` format
//! and applied at runtime by `mirage-core`; nothing in here ships to players.

pub mod clip;
pub mod data;
pub mod eval;
pub mod pgd;
pub mod rng;
