// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-common.md, section "lib.rs"
// ============================================================================

//! Shared error types, traits, and small utilities used across the gindema
//! workspace. This first pass has exactly one thing in it: the checksum
//! that `gindema-guard`'s tamper-detection `Watch` uses, moved here once a
//! second plausible consumer (asset-container integrity checks, program-file
//! sanity checks) made "every crate rolls its own" the wrong call. See
//! `docs/gindema-common.md`'s "Known limits" for why this crate still
//! doesn't have shared error types or traits despite its own description
//! naming them.

mod checksum;

pub use checksum::{checksum, checksums_match};
