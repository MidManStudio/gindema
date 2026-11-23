// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "lib.rs"
// ============================================================================

//! Encrypted asset container, built on `dixscript`'s own AES-256-GCM
//! encryptor and gzip compressor, with gindema-specific defaults: no asset
//! metadata stored at all (no filenames, no timestamps — just bytes),
//! honeypot/decoy slots that are indistinguishable from real ones without
//! the key, and lazy per-asset decryption ([`ContainerReader::open`]
//! decrypts only a small index; [`ContainerReader::read_asset`] decrypts
//! exactly the one asset asked for).
//!
//! This uses `dixscript`'s lower-level [`dixscript::Compiler::DLM::Encryptor::IEncryptor`]/
//! [`dixscript::Compiler::DLM::Compressor::ICompressor`] trait implementations
//! directly, **not** the AST-driven [`dixscript::Compiler::DLM::DLMPipelineExecutor`].
//! That executor is built around compiling a `.dix` script file (reading
//! its `@SECURITY`/`@DLM` sections from parsed source) — the right tool
//! for DixScript's own job, but not a fit for "encrypt these arbitrary
//! asset bytes" with no script involved. The individual encryptor/
//! compressor types are real, usable, documented pieces of the same DLM
//! system; see `docs/gindema-container.md` for the full reasoning.
//!
//! # Example
//!
//! ```
//! use gindema_container::{ContainerBuilder, ContainerKey, ContainerReader};
//!
//! let key = ContainerKey::generate();
//!
//! let mut builder = ContainerBuilder::new();
//! let texture = builder.add_asset(b"pretend this is texture data".to_vec()).unwrap();
//! let sound = builder.add_asset(b"pretend this is sound data".to_vec()).unwrap();
//! let archive = builder.build(&key, /* honeypots */ 4, /* seed */ 0xC0FFEE).unwrap();
//!
//! let reader = ContainerReader::open(&archive, key).unwrap();
//! assert_eq!(reader.asset_count(), 2);
//! assert_eq!(reader.read_asset(texture).unwrap(), b"pretend this is texture data");
//! assert_eq!(reader.read_asset(sound).unwrap(), b"pretend this is sound data");
//! ```

mod builder;
mod crypto;
mod error;
mod index;
mod key;
mod reader;

pub use builder::ContainerBuilder;
pub use error::ContainerError;
pub use key::ContainerKey;
pub use reader::ContainerReader;
