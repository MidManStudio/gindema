// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-bits.md, section "lib.rs"
// ============================================================================

//! Bit-level stream I/O for gindema-vm's bytecode format: pack and unpack
//! arbitrary-width (1..=64 bit) unsigned fields into a byte buffer,
//! most-significant-bit first, with no byte-alignment required between
//! fields. See `docs/gindema-bits.md` for why this is a fresh, self-contained
//! implementation rather than a literal vendoring of mbfa's bitreader.rs/
//! bitwriter.rs, as originally planned.

mod error;
mod reader;
mod writer;

pub use error::BitsError;
pub use reader::BitReader;
pub use writer::BitWriter;
