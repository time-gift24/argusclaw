//! Key material sources for encryption/decryption.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use mac_address::get_mac_address;

use crate::error::CryptoError;

const ARGUSCLAW_MASTER_KEY_PATH: &str = "~/.arguswing/master.key";
const ARGUSCLAW_MASTER_KEY_PATH_ENV: &str = "ARGUSCLAW_MASTER_KEY_PATH";
pub const MASTER_KEY_LEN: usize = 32;

/// Trait for providing key material for encryption/decryption.
pub trait KeyMaterialSource: Send + Sync {
    fn key_material(&self) -> Result<Vec<u8>, CryptoError>;
}

/// Key material source using the host's MAC address.
///
/// # ⚠️ DEPRECATED
///
/// This implementation is **not recommended for production use** because:
/// - MAC addresses can be spoofed
/// - They change when network hardware is replaced
/// - A 6-byte MAC address provides only 48 bits of entropy, insufficient for AES-256
///
/// Use [`FileKeySource`] instead, which generates and manages a proper 256-bit master key.
#[deprecated(
    since = "0.1.0",
    note = "HostMacAddressKeySource is not secure. Use FileKeySource instead."
)]
pub struct HostMacAddressKeySource;

#[allow(deprecated)]
impl KeyMaterialSource for HostMacAddressKeySource {
    fn key_material(&self) -> Result<Vec<u8>, CryptoError> {
        let mac_address = get_mac_address()
            .map_err(|e| CryptoError::HostKeyUnavailable {
                reason: e.to_string(),
            })?
            .ok_or_else(|| CryptoError::HostKeyUnavailable {
                reason: "no MAC address was found on this host".to_string(),
            })?;

        Ok(mac_address.bytes().to_vec())
    }
}

/// Key material source using static bytes (for testing).
pub struct StaticKeySource {
    key_material: Vec<u8>,
}

impl StaticKeySource {
    #[must_use]
    pub fn new(key_material: Vec<u8>) -> Self {
        Self { key_material }
    }
}

impl KeyMaterialSource for StaticKeySource {
    fn key_material(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(self.key_material.clone())
    }
}

/// Key material source using a file (master key).
pub struct FileKeySource {
    configured_path: Option<String>,
}

impl FileKeySource {
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

impl KeyMaterialSource for FileKeySource {
    fn key_material(&self) -> Result<Vec<u8>, CryptoError> {
        let path = resolve_master_key_path(self.configured_path.clone())?;
        load_or_create_master_key(&path)
    }
}

fn resolve_master_key_path(configured: Option<String>) -> Result<PathBuf, CryptoError> {
    let configured = configured
        .or_else(|| env::var(ARGUSCLAW_MASTER_KEY_PATH_ENV).ok())
        .unwrap_or_else(|| ARGUSCLAW_MASTER_KEY_PATH.to_string());

    if let Some(relative_path) = configured.strip_prefix("~/") {
        let home_dir =
            dirs::home_dir().ok_or_else(|| CryptoError::SecretKeyMaterialUnavailable {
                reason: "failed to resolve home directory for master key".to_string(),
            })?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(PathBuf::from(configured))
}

fn load_or_create_master_key(path: &Path) -> Result<Vec<u8>, CryptoError> {
    match fs::read(path) {
        Ok(key_material) => validate_master_key(path, key_material),
        Err(error) if error.kind() == ErrorKind::NotFound => create_master_key(path),
        Err(error) => Err(CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to read `{}`: {error}", path.display()),
        }),
    }
}

fn validate_master_key(path: &Path, key_material: Vec<u8>) -> Result<Vec<u8>, CryptoError> {
    if key_material.is_empty() {
        return replace_empty_master_key(path);
    }

    if key_material.len() != MASTER_KEY_LEN {
        return Err(CryptoError::SecretKeyMaterialUnavailable {
            reason: format!(
                "expected `{}` to contain {MASTER_KEY_LEN} bytes, found {}",
                path.display(),
                key_material.len()
            ),
        });
    }

    Ok(key_material)
}

fn create_master_key(path: &Path) -> Result<Vec<u8>, CryptoError> {
    let parent = path
        .parent()
        .ok_or_else(|| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!(
                "master key path `{}` has no parent directory",
                path.display()
            ),
        })?;
    fs::create_dir_all(parent)
        .map_err(|error| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to create `{}`: {error}", parent.display()),
        })?;

    let key_material = generate_master_key()?;

    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => write_master_key_file(path, file, &key_material),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => load_or_create_master_key(path),
        Err(error) => Err(CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to create `{}`: {error}", path.display()),
        }),
    }
}

fn replace_empty_master_key(path: &Path) -> Result<Vec<u8>, CryptoError> {
    let key_material = generate_master_key()?;
    let file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to recreate `{}`: {error}", path.display()),
        })?;

    write_master_key_file(path, file, &key_material)
}

fn generate_master_key() -> Result<Vec<u8>, CryptoError> {
    use ring::rand::{SecureRandom, SystemRandom};

    let mut key_material = vec![0_u8; MASTER_KEY_LEN];
    SystemRandom::new()
        .fill(&mut key_material)
        .map_err(|error| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to generate master key bytes: {error}"),
        })?;

    Ok(key_material)
}

fn write_master_key_file(
    path: &Path,
    mut file: fs::File,
    key_material: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    file.write_all(key_material)
        .map_err(|error| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to write `{}`: {error}", path.display()),
        })?;
    file.sync_all()
        .map_err(|error| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to flush `{}`: {error}", path.display()),
        })?;
    set_master_key_permissions(path)?;
    Ok(key_material.to_vec())
}

#[cfg(unix)]
fn set_master_key_permissions(path: &Path) -> Result<(), CryptoError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| CryptoError::SecretKeyMaterialUnavailable {
            reason: format!("failed to set permissions on `{}`: {error}", path.display()),
        })
}

#[cfg(not(unix))]
fn set_master_key_permissions(_path: &Path) -> Result<(), CryptoError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn file_key_source_persists_a_stable_master_key() {
        let temp_dir = tempdir().expect("tempdir should exist");
        let path = temp_dir.path().join("master.key");
        let source = FileKeySource::new(path.display().to_string());

        let first = source.key_material().expect("master key should be created");
        let second = source.key_material().expect("master key should be reused");

        assert_eq!(first.len(), MASTER_KEY_LEN);
        assert_eq!(first, second);
    }

    #[test]
    fn file_key_source_recreates_an_empty_master_key_file() {
        let temp_dir = tempdir().expect("tempdir should exist");
        let path = temp_dir.path().join("master.key");
        fs::write(&path, []).expect("empty master key file should be created");

        let source = FileKeySource::new(path.display().to_string());
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
