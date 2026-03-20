//! argus-crypto - Encryption utilities for ArgusWing.

pub mod cipher;
pub mod error;
pub mod key_source;

pub use cipher::{Cipher, EncryptedSecret};
pub use error::CryptoError;
pub use key_source::{FileKeySource, KeyMaterialSource, StaticKeySource};
