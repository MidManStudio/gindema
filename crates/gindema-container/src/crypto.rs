// ============================================================================
// NOTICE: Full documentation, design decisions, and fix history for this file
// live in docs/gindema-container.md, section "crypto.rs"
// ============================================================================

use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use dixscript::Compiler::DLM::Compressor::{GzipCompressor, ICompressor};
use dixscript::Compiler::DLM::Encryptor::{Aes256Encryptor, IEncryptor};

use crate::error::ContainerError;
use crate::key::ContainerKey;

/// Compresses then encrypts `plaintext` into one self-contained blob: a
/// fresh `Aes256Encryptor`, given `key` and the caller-supplied `iv`,
/// prepends its own 12-byte IV to the GCM ciphertext — so the blob needs
/// nothing alongside it to decrypt later except `key` itself.
///
/// `iv` is the caller's responsibility to make unique per call under the
/// same `key` (AES-GCM security depends on never reusing a nonce under
/// the same key) — this function itself has no opinion on where `iv` came
/// from. See `builder.rs` for how this crate generates one.
pub(crate) fn seal(
    key: &ContainerKey,
    iv: &[u8; 12],
    plaintext: &[u8],
) -> Result<Vec<u8>, ContainerError> {
    let compressed = GzipCompressor::new()
        .compress(plaintext)
        .map_err(ContainerError::Compress)?;

    let mut config = HashMap::new();
    config.insert("key_data".to_string(), key.to_base64());
    config.insert("iv".to_string(), BASE64.encode(iv));

    let mut encryptor = Aes256Encryptor::new(None);
    encryptor.initialize(config);
    encryptor
        .encrypt(&compressed)
        .map_err(ContainerError::Encrypt)
}

/// The inverse of [`seal`]: decrypts, then decompresses. Needs only `key`
/// — the IV used to encrypt `ciphertext` is read back out of
/// `ciphertext`'s own first 12 bytes by `Aes256Encryptor::decrypt`
/// itself, not supplied separately.
pub(crate) fn open(key: &ContainerKey, ciphertext: &[u8]) -> Result<Vec<u8>, ContainerError> {
    let mut config = HashMap::new();
    config.insert("key_data".to_string(), key.to_base64());

    let mut encryptor = Aes256Encryptor::new(None);
    encryptor.initialize(config);
    let compressed = encryptor
        .decrypt(ciphertext)
        .map_err(ContainerError::Decrypt)?;

    GzipCompressor::new()
        .decompress(&compressed)
        .map_err(ContainerError::Decompress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_then_open_recovers_the_original_bytes() {
        let key = ContainerKey::generate();
        let iv = [1u8; 12];
        let plaintext = b"a reasonably real-looking asset payload, repeated ".repeat(20);
        let sealed = seal(&key, &iv, &plaintext).unwrap();
        let opened = open(&key, &sealed).unwrap();
        assert_eq!(opened, plaintext);
    }

    #[test]
    fn opening_with_the_wrong_key_fails_rather_than_returning_garbage() {
        let key = ContainerKey::generate();
        let wrong_key = ContainerKey::generate();
        let sealed = seal(&key, &[2u8; 12], b"secret asset bytes").unwrap();
        assert!(matches!(
            open(&wrong_key, &sealed),
            Err(ContainerError::Decrypt(_))
        ));
    }

    #[test]
    fn tampered_ciphertext_fails_to_authenticate() {
        let key = ContainerKey::generate();
        let mut sealed = seal(&key, &[3u8; 12], b"asset bytes worth protecting").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0xFF;
        assert!(matches!(
            open(&key, &sealed),
            Err(ContainerError::Decrypt(_))
        ));
    }

    #[test]
    fn different_ivs_for_the_same_plaintext_and_key_produce_different_blobs() {
        let key = ContainerKey::generate();
        let plaintext = b"identical payload, different nonce";
        let a = seal(&key, &[4u8; 12], plaintext).unwrap();
        let b = seal(&key, &[5u8; 12], plaintext).unwrap();
        assert_ne!(a, b);
    }
}
