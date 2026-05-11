use std::net::SocketAddr;
use std::path::PathBuf;

use argus_crypto::{Cipher, FileKeySource};
use argus_server::server_config::{EncryptedConfigValue, ServerConfig};

#[test]
fn server_config_defaults_to_localhost_3000() {
    let config =
        ServerConfig::from_env_values(None, None, None, None, None, None, None, None, None, None)
            .expect("default bind address should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 3000)));
}

#[test]
fn server_config_accepts_argus_server_addr() {
    let config = ServerConfig::from_env_values(
        Some("127.0.0.1:4181"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("custom addr should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 4181)));
}

#[test]
fn server_config_accepts_web_dist_dir() {
    let config = ServerConfig::from_env_values(
        Some("127.0.0.1:4181"),
        Some("/opt/arguswing/web"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("custom config should parse");
    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 4181)));
    assert_eq!(
        config.web_dist_dir,
        Some(PathBuf::from("/opt/arguswing/web"))
    );
}

#[test]
fn server_config_parses_toml_and_decrypts_secret_fields() {
    let dir = tempfile::tempdir().expect("tempdir should exist");
    let master_key_path = dir.path().join("master.key");
    let cipher = Cipher::new(FileKeySource::new(master_key_path.display().to_string()));
    let database_url = EncryptedConfigValue::encrypt_with_cipher(
        &cipher,
        "postgres://argus:secret@127.0.0.1:5432/argus",
    )
    .expect("database url should encrypt");
    let client_secret = EncryptedConfigValue::encrypt_with_cipher(&cipher, "oauth-secret")
        .expect("oauth secret should encrypt");

    let toml = format!(
        r#"
        [server]
        bind_addr = "0.0.0.0:3010"
        web_dist_dir = "/opt/arguswing/web"

        [database]
        url = {{ encrypted = "{}", nonce = "{}" }}

        [trace]
        dir = "/opt/arguswing/traces"

        [crypto]
        master_key_path = "{}"

        [auth]
        dev_enabled = false

        [auth.oauth]
        enabled = true
        base_url = "https://auth.example.test"
        client_id = "argus-client"
        client_secret = {{ encrypted = "{}", nonce = "{}" }}
        redirect_uri = "https://argus.example.test/auth/callback"
        scope = "base.profile"
        cookie_secure = true

        [logging]
        level = "debug"
        file_path = "/var/log/arguswing/server.log"
        "#,
        database_url.encrypted,
        database_url.nonce,
        master_key_path.display(),
        client_secret.encrypted,
        client_secret.nonce
    );

    let config = ServerConfig::from_toml_str(&toml).expect("toml config should parse");

    assert_eq!(config.bind_addr, SocketAddr::from(([0, 0, 0, 0], 3010)));
    assert_eq!(
        config.database_url,
        "postgres://argus:secret@127.0.0.1:5432/argus"
    );
    assert_eq!(
        config.auth.oauth.client_secret.as_deref(),
        Some("oauth-secret")
    );
    assert_eq!(config.logging.level.as_deref(), Some("debug"));
    assert_eq!(
        config.logging.file_path,
        Some(PathBuf::from("/var/log/arguswing/server.log"))
    );
}

#[test]
fn encrypted_config_value_debug_redacts_ciphertext() {
    let value = EncryptedConfigValue {
        encrypted: "ciphertext".to_string(),
        nonce: "nonce".to_string(),
    };

    let debug = format!("{value:?}");

    assert!(!debug.contains("ciphertext"));
    assert!(!debug.contains("nonce"));
    assert!(debug.contains("REDACTED"));
}
