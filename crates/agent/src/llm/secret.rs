use std::sync::Arc;

use mac_address::get_mac_address;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, KeyType, Salt};
use ring::rand::{SecureRandom, SystemRandom};

use crate::db::DbError;
use crate::db::llm::SecretString;

const ARGUSCLAW_KEY_SALT: &[u8] = b"argusclaw.llm.api-key.salt.v1";
const ARGUSCLAW_KEY_INFO: &[u8] = b"argusclaw.llm.api-key.info.v1";
const NONCE_LEN: usize = 12;

pub trait KeyMaterialSource: Send + Sync {
    fn key_material(&self) -> Result<Vec<u8>, DbError>;
}

pub struct HostMacAddressKeyMaterialSource;

impl KeyMaterialSource for HostMacAddressKeyMaterialSource {
    fn key_material(&self) -> Result<Vec<u8>, DbError> {
        let mac_address = get_mac_address()
            .map_err(|e| DbError::HostKeyUnavailable {
                reason: e.to_string(),
            })?
            .ok_or_else(|| DbError::HostKeyUnavailable {
                reason: "no MAC address was found on this host".to_string(),
            })?;

        Ok(mac_address.bytes().to_vec())
    }
}

pub struct StaticKeyMaterialSource {
    key_material: Vec<u8>,
}

impl StaticKeyMaterialSource {
    #[must_use]
    pub fn new(key_material: Vec<u8>) -> Self {
        Self { key_material }
    }
}

impl KeyMaterialSource for StaticKeyMaterialSource {
    fn key_material(&self) -> Result<Vec<u8>, DbError> {
        Ok(self.key_material.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Clone)]
pub struct ApiKeyCipher {
    key_source: Arc<dyn KeyMaterialSource>,
}

impl ApiKeyCipher {
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

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedSecret, DbError> {
        let key = self.derive_key()?;
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce_bytes).map_err(|e| {
            DbError::SecretEncryptionFailed {
                reason: e.to_string(),
            }
        })?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut ciphertext = plaintext.as_bytes().to_vec();
        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
            .map_err(|_| DbError::SecretEncryptionFailed {
                reason: "aes-256-gcm seal failed".to_string(),
            })?;

        Ok(EncryptedSecret {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })
    }

    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<SecretString, DbError> {
        let key = self.derive_key()?;
        let nonce_array: [u8; NONCE_LEN] =
            nonce
                .try_into()
                .map_err(|_| DbError::SecretDecryptionFailed {
                    reason: "invalid nonce length".to_string(),
                })?;

        let mut plaintext = ciphertext.to_vec();
        let decrypted = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_array),
                Aad::empty(),
                &mut plaintext,
            )
            .map_err(|_| DbError::SecretDecryptionFailed {
                reason: "aes-256-gcm open failed; key material may have changed".to_string(),
            })?;
        let value =
            String::from_utf8(decrypted.to_vec()).map_err(|e| DbError::SecretDecryptionFailed {
                reason: e.to_string(),
            })?;

        Ok(SecretString::new(value))
    }

    fn derive_key(&self) -> Result<LessSafeKey, DbError> {
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
            DbError::SecretEncryptionFailed {
                reason: "hkdf expansion failed".to_string(),
            }
        })?;

        let mut key_bytes = [0_u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| DbError::SecretEncryptionFailed {
                reason: "hkdf fill failed".to_string(),
            })?;

        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| {
            DbError::SecretEncryptionFailed {
                reason: "failed to initialize aes-256-gcm key".to_string(),
            }
        })?;

        Ok(LessSafeKey::new(unbound_key))
    }
}
