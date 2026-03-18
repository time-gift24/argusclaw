//! API key encryption/decryption using host-bound key material.
//!
//! This module provides secure storage for API keys by encrypting them with
//! a key derived from the host's MAC address or a master key file.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mac_address::get_mac_address;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::hkdf::{HKDF_SHA256, KeyType, Salt};
use ring::rand::{SecureRandom, SystemRandom};

use argus_protocol::SecretString;

const ARGUSCLAW_KEY_SALT: &[u8] = b"argusclaw.llm.api-key.salt.v1";
const ARGUSCLAW_KEY_INFO: &[u8] = b"argusclaw.llm.api-key.info.v1";
const ARGUSCLAW_MASTER_KEY_PATH: &str = "~/.arguswing/master.key";
const ARGUSCLAW_MASTER_KEY_PATH_ENV: &str = "ARGUSCLAW_MASTER_KEY_PATH";
const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Error type for secret operations.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("host key material is unavailable: {reason}")]
    HostKeyUnavailable { reason: String },

    #[error("secret key material is unavailable: {reason}")]
    SecretKeyMaterialUnavailable { reason: String },

    #[error("failed to encrypt secret: {reason}")]
    SecretEncryptionFailed { reason: String },

    #[error("failed to decrypt secret: {reason}")]
    SecretDecryptionFailed { reason: String },
}

/// Trait for providing key material for encryption/decryption.
pub trait KeyMaterialSource: Send + Sync {
    fn key_material(&self) -> Result<Vec<u8>, SecretError>;
}

/// Key material source using the host's MAC address.
pub struct HostMacAddressKeyMaterialSource;

impl KeyMaterialSource for HostMacAddressKeyMaterialSource {
    fn key_material(&self) -> Result<Vec<u8>, SecretError> {
        let mac_address = get_mac_address()
            .map_err(|e| SecretError::HostKeyUnavailable {
                reason: e.to_string(),
            })?
            .ok_or_else(|| SecretError::HostKeyUnavailable {
                reason: "no MAC address was found on this host".to_string(),
            })?;

        Ok(mac_address.bytes().to_vec())
    }
}

/// Key material source using static bytes (for testing).
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
    fn key_material(&self) -> Result<Vec<u8>, SecretError> {
        Ok(self.key_material.clone())
    }
}

/// Key material source using a file (master key).
pub struct FileKeyMaterialSource {
    configured_path: Option<String>,
}

impl FileKeyMaterialSource {
    #[must_use]
    pub fn new(configured_path: impl Into<String>) -> Self {
        Self {
            configured_path: Some(configured_path.into()),
        }
    }

    #[must_use]
    pub fn from_env_or_default() -> Self {
        Self {
            configured_path: None,
        }
    }
}

impl KeyMaterialSource for FileKeyMaterialSource {
    fn key_material(&self) -> Result<Vec<u8>, SecretError> {
        let path = resolve_master_key_path(self.configured_path.clone())?;
        load_or_create_master_key(&path)
    }
}

/// Encrypted secret with nonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Cipher for encrypting/decrypting API keys.
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

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedSecret, SecretError> {
        let key = self.derive_key()?;
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce_bytes).map_err(|e| {
            SecretError::SecretEncryptionFailed {
                reason: e.to_string(),
            }
        })?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut ciphertext = plaintext.as_bytes().to_vec();
        key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
            .map_err(|_| SecretError::SecretEncryptionFailed {
                reason: "aes-256-gcm seal failed".to_string(),
            })?;

        Ok(EncryptedSecret {
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })
    }

    pub fn decrypt(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<SecretString, SecretError> {
        let key = self.derive_key()?;
        let nonce_array: [u8; NONCE_LEN] =
            nonce
                .try_into()
                .map_err(|_| SecretError::SecretDecryptionFailed {
                    reason: "invalid nonce length".to_string(),
                })?;

        let mut plaintext = ciphertext.to_vec();
        let decrypted = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_array),
                Aad::empty(),
                &mut plaintext,
            )
            .map_err(|_| SecretError::SecretDecryptionFailed {
                reason: "aes-256-gcm open failed; key material may have changed".to_string(),
            })?;
        let value = String::from_utf8(decrypted.to_vec()).map_err(|e| {
            SecretError::SecretDecryptionFailed {
                reason: e.to_string(),
            }
        })?;

        Ok(SecretString::new(value))
    }

    fn derive_key(&self) -> Result<LessSafeKey, SecretError> {
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
            SecretError::SecretEncryptionFailed {
                reason: "hkdf expansion failed".to_string(),
            }
        })?;

        let mut key_bytes = [0_u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| SecretError::SecretEncryptionFailed {
                reason: "hkdf fill failed".to_string(),
            })?;

        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes).map_err(|_| {
            SecretError::SecretEncryptionFailed {
                reason: "failed to initialize aes-256-gcm key".to_string(),
            }
        })?;

        Ok(LessSafeKey::new(unbound_key))
    }
}

fn resolve_master_key_path(configured: Option<String>) -> Result<PathBuf, SecretError> {
    let configured = configured
        .or_else(|| env::var(ARGUSCLAW_MASTER_KEY_PATH_ENV).ok())
        .unwrap_or_else(|| ARGUSCLAW_MASTER_KEY_PATH.to_string());

    if let Some(relative_path) = configured.strip_prefix("~/") {
        let home_dir =
            dirs::home_dir().ok_or_else(|| SecretError::SecretKeyMaterialUnavailable {
                reason: "failed to resolve home directory for master key".to_string(),
            })?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(PathBuf::from(configured))
}

fn load_or_create_master_key(path: &Path) -> Result<Vec<u8>, SecretError> {
    match fs::read(path) {
        Ok(key_material) => validate_master_key(path, key_material),
        Err(error) if error.kind() == ErrorKind::NotFound => create_master_key(path),
        Err(error) => Err(SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to read `{}`: {error}", path.display()),
        }),
    }
}

fn validate_master_key(path: &Path, key_material: Vec<u8>) -> Result<Vec<u8>, SecretError> {
    if key_material.is_empty() {
        return replace_empty_master_key(path);
    }

    if key_material.len() != MASTER_KEY_LEN {
        return Err(SecretError::SecretKeyMaterialUnavailable {
            reason: format!(
                "expected `{}` to contain {MASTER_KEY_LEN} bytes, found {}",
                path.display(),
                key_material.len()
            ),
        });
    }

    Ok(key_material)
}

fn create_master_key(path: &Path) -> Result<Vec<u8>, SecretError> {
    let parent = path
        .parent()
        .ok_or_else(|| SecretError::SecretKeyMaterialUnavailable {
            reason: format!(
                "master key path `{}` has no parent directory",
                path.display()
            ),
        })?;
    fs::create_dir_all(parent).map_err(|error| SecretError::SecretKeyMaterialUnavailable {
        reason: format!("failed to create `{}`: {error}", parent.display()),
    })?;

    let key_material = generate_master_key()?;

    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => write_master_key_file(path, file, &key_material),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => load_or_create_master_key(path),
        Err(error) => Err(SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to create `{}`: {error}", path.display()),
        }),
    }
}

fn replace_empty_master_key(path: &Path) -> Result<Vec<u8>, SecretError> {
    let key_material = generate_master_key()?;
    let file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to recreate `{}`: {error}", path.display()),
        })?;

    write_master_key_file(path, file, &key_material)
}

fn generate_master_key() -> Result<Vec<u8>, SecretError> {
    let mut key_material = vec![0_u8; MASTER_KEY_LEN];
    SystemRandom::new()
        .fill(&mut key_material)
        .map_err(|error| SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to generate master key bytes: {error}"),
        })?;

    Ok(key_material)
}

fn write_master_key_file(
    path: &Path,
    mut file: fs::File,
    key_material: &[u8],
) -> Result<Vec<u8>, SecretError> {
    file.write_all(key_material)
        .map_err(|error| SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to write `{}`: {error}", path.display()),
        })?;
    file.sync_all()
        .map_err(|error| SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to flush `{}`: {error}", path.display()),
        })?;
    set_master_key_permissions(path)?;
    Ok(key_material.to_vec())
}

#[cfg(unix)]
fn set_master_key_permissions(path: &Path) -> Result<(), SecretError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        SecretError::SecretKeyMaterialUnavailable {
            reason: format!("failed to set permissions on `{}`: {error}", path.display()),
        }
    })
}

#[cfg(not(unix))]
fn set_master_key_permissions(_path: &Path) -> Result<(), SecretError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn api_key_cipher_round_trips_with_fixed_key_material() {
        let cipher = ApiKeyCipher::new(StaticKeyMaterialSource::new(b"fixed-test-key".to_vec()));
        let encrypted = cipher.encrypt("sk-secret").expect("secret should encrypt");
        let decrypted = cipher
            .decrypt(&encrypted.nonce, &encrypted.ciphertext)
            .expect("secret should decrypt");

        assert_eq!(decrypted.expose_secret(), "sk-secret");
        assert_ne!(encrypted.ciphertext, b"sk-secret");
    }

    #[test]
    fn file_key_material_source_persists_a_stable_master_key() {
        let temp_dir = tempdir().expect("tempdir should exist");
        let path = temp_dir.path().join("master.key");
        let source = FileKeyMaterialSource::new(path.display().to_string());

        let first = source.key_material().expect("master key should be created");
        let second = source.key_material().expect("master key should be reused");

        assert_eq!(first.len(), MASTER_KEY_LEN);
        assert_eq!(first, second);
    }

    #[test]
    fn file_key_material_source_recreates_an_empty_master_key_file() {
        let temp_dir = tempdir().expect("tempdir should exist");
        let path = temp_dir.path().join("master.key");
        fs::write(&path, []).expect("empty master key file should be created");

        let source = FileKeyMaterialSource::new(path.display().to_string());
        let key_material = source
            .key_material()
            .expect("empty master key should be recreated");

        assert_eq!(key_material.len(), MASTER_KEY_LEN);
        assert_eq!(
            fs::read(&path).expect("master key should be readable"),
            key_material,
        );
    }
}
