// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "lib.rs"
// ============================================================================

//! Encrypted asset container. Wraps dixscript's DLM pipeline with gindema-specific defaults: stripped metadata, honeypot blocks, lazy per-chunk decrypt.

// TODO: scaffold stage only, not yet implemented.
