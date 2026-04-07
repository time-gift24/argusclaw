//! Server user types for OAuth2-based authentication.
//!
//! These types model the server-side user identity:
//! - `OAuth2Identity`: identity claims returned by an OAuth2 provider
//! - `UserRecord`: persisted user record in the server database

use serde::{Deserialize, Serialize};

use crate::ids::ProviderId;

/// Identity claims returned by an OAuth2 provider after code exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuth2Identity {
    /// Subject identifier from the external provider (unique per provider).
    pub external_subject: String,
    /// Account identifier (e.g., email).
    pub account: String,
    /// Human-readable display name.
    pub display_name: String,
}

/// Persisted user record for the server product.
///
/// Created or updated via OAuth2 upsert (by `external_subject`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    /// Unique database-assigned user ID.
    pub id: i64,
    /// Subject identifier from the external OAuth2 provider.
    pub external_subject: String,
    /// Account identifier (e.g., email).
    pub account: String,
    /// Human-readable display name.
    pub display_name: String,
}

/// Provider token-exchange credential stored server-side.
///
/// Holds encrypted credentials used by token-exchange LLM providers.
/// The server manages these as secrets; ordinary users never access them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTokenCredential {
    /// The provider these credentials belong to.
    pub provider_id: ProviderId,
    /// Username for token exchange.
    pub username: String,
    /// Encrypted password ciphertext.
    pub ciphertext: Vec<u8>,
    /// Nonce used for decryption.
    pub nonce: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth2_identity_round_trips_through_json() {
        let identity = OAuth2Identity {
            external_subject: "google-oauth2|1234567890".to_string(),
            account: "user@example.com".to_string(),
            display_name: "Test User".to_string(),
        };
        let json = serde_json::to_string(&identity).expect("serialize should succeed");
        let deserialized: OAuth2Identity =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(deserialized, identity);
    }

    #[test]
    fn user_record_round_trips_through_json() {
        let record = UserRecord {
            id: 42,
            external_subject: "google-oauth2|1234567890".to_string(),
            account: "user@example.com".to_string(),
            display_name: "Test User".to_string(),
        };
        let json = serde_json::to_string(&record).expect("serialize should succeed");
        let deserialized: UserRecord =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(deserialized, record);
    }

    #[test]
    fn provider_token_credential_round_trips_through_json() {
        let credential = ProviderTokenCredential {
            provider_id: ProviderId::new(1),
            username: "service-account".to_string(),
            ciphertext: vec![1, 2, 3, 4],
            nonce: vec![5, 6, 7, 8],
        };
        let json = serde_json::to_string(&credential).expect("serialize should succeed");
        let deserialized: ProviderTokenCredential =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(deserialized, credential);
    }
}
