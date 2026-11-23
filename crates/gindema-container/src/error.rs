// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "error.rs"
// ============================================================================

use std::fmt;

/// Everything that can go wrong building or reading a gindema-container
/// archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    /// [`crate::ContainerBuilder::add_asset`] was given zero-byte data —
    /// not supported, since dixscript's own compressor and encryptor both
    /// reject empty input outright. Not expected to matter for real game
    /// assets.
    EmptyAsset,
    /// A key string wasn't valid base64, or didn't decode to exactly 32
    /// bytes (an AES-256 key).
    InvalidKey(String),
    /// The archive didn't start with the expected 4-byte magic.
    InvalidMagic,
    /// The archive declared a format version this crate doesn't know how
    /// to read.
    UnsupportedVersion(u16),
    /// The archive ended before a complete header, index, or slot could be
    /// read.
    UnexpectedEof,
    /// The decrypted index didn't parse as a valid slot list — implies a
    /// corrupted archive or the wrong key (which would usually surface as
    /// [`ContainerError::Decrypt`] first, from the index blob itself
    /// failing to authenticate, but a key that happens to produce garbage
    /// that still passes GCM authentication by chance is cryptographically
    /// implausible enough not to special-case further).
    CorruptIndex,
    /// No slot in the archive holds the requested asset order.
    InvalidAssetId(u32),
    /// dixscript's compressor rejected the data (message from dixscript).
    Compress(String),
    /// dixscript's compressor couldn't decompress the data (message from
    /// dixscript) — a corrupted archive, most likely.
    Decompress(String),
    /// dixscript's encryptor rejected the data (message from dixscript).
    Encrypt(String),
    /// dixscript's encryptor couldn't decrypt the data (message from
    /// dixscript) — the wrong key or a tampered/corrupted archive; AES-GCM
    /// is authenticated, so these two cases aren't distinguishable from
    /// the failure alone.
    Decrypt(String),
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAsset => write!(f, "cannot store a zero-byte asset"),
            Self::InvalidKey(msg) => write!(f, "invalid container key: {msg}"),
            Self::InvalidMagic => write!(f, "not a gindema-container archive (bad magic)"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported archive version {v}"),
            Self::UnexpectedEof => write!(f, "archive ended unexpectedly"),
            Self::CorruptIndex => write!(f, "archive index is corrupt"),
            Self::InvalidAssetId(id) => write!(f, "no asset with order {id} in this archive"),
            Self::Compress(msg) => write!(f, "compression failed: {msg}"),
            Self::Decompress(msg) => write!(f, "decompression failed: {msg}"),
            Self::Encrypt(msg) => write!(f, "encryption failed: {msg}"),
            Self::Decrypt(msg) => write!(
                f,
                "decryption failed (wrong key, or corrupted/tampered data): {msg}"
            ),
        }
    }
}

impl std::error::Error for ContainerError {}
