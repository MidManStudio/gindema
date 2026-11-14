// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-vm.md, section "lib.rs"
// ============================================================================

//! Custom stack-based bytecode format and interpreter. The core virtual machine the rest of the gindema workspace builds on.

#![allow(unsafe_code)] // see docs/gindema-vm.md for the safety rationale

// TODO: scaffold stage only, not yet implemented.
