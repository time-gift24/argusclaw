//! AES-256-GCM encryption/decryption utilities.

use std::sync::Arc;

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, KeyType, Salt};
use ring::rand::{SecureRandom, SystemRandom};

use argus_protocol::SecretString;

use crate::error::CryptoError;
use crate::key_source::KeyMaterialSource;

const ARGUSCLAW_KEY_SALT: &[u8] = b"argusclaw.llm.api-key.salt.v1";
const ARGUSCLAW_KEY_INFO: &[u8] = b"argusclaw.llm.api-key.info.v1";
const NONCE_LEN: usize = 12;

/// Encrypted secret with nonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Cipher for encrypting/decrypting secrets.
#[derive(Clone)]
pub struct Cipher {
    key_source: Arc<dyn KeyMaterialSource>,
}

impl Cipher {
    #[must_use]
    pub fn new(key_source: impl KeyMaterialSource + 'static) -> Self {
        Self {
            key_source: Arc::new(key_source),
        }
    }

    #[must_use]
    pub fn new_arc(key_source: Arc<dyn KeyMaterialSource>) -> Self {
        Self { key_source }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedSecret, CryptoError> {
        let key = self.derive_key()?;
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce_bytes).map_err(|e| {
            CryptoError::SecretEncryptionFailed {
                reason: e.to_string(),
            }
        })?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut ciphertext = plaintext.as_bytes().to_vec();
        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
            .map_err(|_| CryptoError::SecretEncryptionFailed {
                reason: "aes-256-gcm seal failed".to_string(),
            })?;

        Ok(EncryptedSecret {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })
    }

    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<SecretString, CryptoError> {
        let key = self.derive_key()?;
        let nonce_array: [u8; NONCE_LEN] =
            nonce
                .try_into()
                .map_err(|_| CryptoError::SecretDecryptionFailed {
                    reason: "invalid nonce length".to_string(),
                })?;

        let mut plaintext = ciphertext.to_vec();
        let decrypted = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_array),
                Aad::empty(),
                &mut plaintext,
            )
            .map_err(|_| CryptoError::SecretDecryptionFailed {
                reason: "aes-256-gcm open failed; key material may have changed".to_string(),
            })?;
        let value = String::from_utf8(decrypted.to_vec()).map_err(|e| {
            CryptoError::SecretDecryptionFailed {
                reason: e.to_string(),
            }
        })?;

        Ok(SecretString::new(value))
    }

    fn derive_key(&self) -> Result<LessSafeKey, CryptoError> {
        struct Aes256Key;

        impl KeyType for Aes256Key {
            fn len(&self) -> usize {
                32
            }
        }

        let key_material = self.key_source.key_material()?;
        let salt = Salt::new(HKDF_SHA256, ARGUSCLAW_KEY_SALT);
        let prk = salt.extract(&key_material);
        let okm = prk.expand(&[ARGUSCLAW_KEY_INFO], Aes256Key).map_err(|_| {
            CryptoError::SecretEncryptionFailed {
                reason: "hkdf expansion failed".to_string(),
            }
        })?;

        let mut key_bytes = [0_u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| CryptoError::SecretEncryptionFailed {
                reason: "hkdf fill failed".to_string(),
            })?;

        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| {
            CryptoError::SecretEncryptionFailed {
                reason: "failed to initialize aes-256-gcm key".to_string(),
            }
        })?;

        Ok(LessSafeKey::new(unbound_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_source::StaticKeySource;

    #[test]
    fn cipher_round_trips_with_fixed_key_material() {
        let cipher = Cipher::new(StaticKeySource::new(b"fixed-test-key".to_vec()));
        let encrypted = cipher.encrypt("sk-secret").expect("secret should encrypt");
        let decrypted = cipher
            .decrypt(&encrypted.nonce, &encrypted.ciphertext)
            .expect("secret should decrypt");

        assert_eq!(decrypted.expose_secret(), "sk-secret");
        assert_ne!(encrypted.ciphertext, b"sk-secret");
    }
}
