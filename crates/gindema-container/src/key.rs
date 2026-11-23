// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "key.rs"
// ============================================================================

use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use dixscript::Compiler::DLM::Encryptor::{Aes256Encryptor, IEncryptor};

use crate::error::ContainerError;

/// A 256-bit AES key for one archive. Generated fresh per archive — this
/// crate never derives a key from a password or reuses one across
/// archives; key distribution (embedding it in a build, deriving it some
/// other way) is entirely up to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerKey(Vec<u8>);

impl ContainerKey {
    /// Generates a fresh random key.
    ///
    /// Goes through `dixscript`'s own `Aes256Encryptor` rather than
    /// reaching for a CSPRNG directly in this crate: `initialize` with an
    /// empty config takes its "forward pipeline, no existing key" branch,
    /// which generates one via `OsRng` internally and reports it back
    /// through `get_metadata()`'s `key_data` entry. Real key material
    /// should come from a real CSPRNG, and dixscript's own already is one
    /// — there was no reason to duplicate that here.
    pub fn generate() -> Self {
        let mut encryptor = Aes256Encryptor::new(None);
        encryptor.initialize(HashMap::new());
        let metadata = encryptor.get_metadata();
        let encoded = metadata.get("key_data").expect(
            "Aes256Encryptor::initialize with an empty config always generates a key and \
             reports it via get_metadata()'s key_data entry",
        );
        let bytes = BASE64.decode(encoded).expect(
            "dixscript base64-encodes the key it just generated internally; decoding its own \
             encoding cannot fail",
        );
        Self(bytes)
    }

    /// Encodes this key as base64, for storing or embedding it somewhere.
    pub fn to_base64(&self) -> String {
        BASE64.encode(&self.0)
    }

    /// Parses a key from base64.
    ///
    /// # Errors
    /// [`ContainerError::InvalidKey`] if `s` isn't valid base64, or
    /// doesn't decode to exactly 32 bytes.
    pub fn from_base64(s: &str) -> Result<Self, ContainerError> {
        let bytes = BASE64
            .decode(s)
            .map_err(|e| ContainerError::InvalidKey(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(ContainerError::InvalidKey(format!(
                "expected a 32-byte AES-256 key, got {} bytes",
                bytes.len()
            )));
        }
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_round_trip_through_base64() {
        let key = ContainerKey::generate();
        let restored = ContainerKey::from_base64(&key.to_base64()).unwrap();
        assert_eq!(key, restored);
    }

    #[test]
    fn two_generated_keys_are_different() {
        assert_ne!(ContainerKey::generate(), ContainerKey::generate());
    }

    #[test]
    fn from_base64_rejects_invalid_base64_and_wrong_length() {
        assert!(ContainerKey::from_base64("not valid base64!!!").is_err());
        assert!(ContainerKey::from_base64(
            &base64::engine::general_purpose::STANDARD.encode(b"too short")
        )
        .is_err());
    }
}
