// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-guard.md, section "lib.rs"
// ============================================================================

//! Runtime anti-tamper watchdog. Timing and entropy checks, pollution-state responses instead of crashing. Applied selectively, never the whole game loop.

#![allow(unsafe_code)] // see docs/gindema-guard.md for the safety rationale

// TODO: scaffold stage only, not yet implemented.
