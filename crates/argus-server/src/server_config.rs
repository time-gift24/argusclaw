use std::fmt;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use argus_crypto::{Cipher, CryptoError, FileKeySource};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use thiserror::Error;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";
const DEFAULT_MASTER_KEY_PATH: &str = "~/.arguswing/master.key";
pub const DEPLOY_CONFIG_PATH: &str = "/etc/arguswing/arguswing.toml";
pub const LOCAL_CONFIG_PATH: &str = "./arguswing.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub web_dist_dir: Option<PathBuf>,
    pub database_url: String,
    pub trace_dir: PathBuf,
    pub master_key_path: PathBuf,
    pub auth: AuthSettings,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSettings {
    pub dev_enabled: bool,
    pub oauth: OAuthSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthSettings {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub base_url: Option<String>,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub logout_url: Option<String>,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub cookie_secure: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingConfig {
    pub level: Option<String>,
    pub file_path: Option<PathBuf>,
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct EncryptedConfigValue {
    pub encrypted: String,
    pub nonce: String,
}

impl fmt::Debug for EncryptedConfigValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EncryptedConfigValue(REDACTED)")
    }
}

impl EncryptedConfigValue {
    pub fn encrypt_with_cipher(cipher: &Cipher, value: &str) -> Result<Self, ServerConfigError> {
        let encrypted = cipher.encrypt(value)?;
        Ok(Self {
            encrypted: STANDARD.encode(encrypted.ciphertext),
            nonce: STANDARD.encode(encrypted.nonce),
        })
    }

    pub fn encrypt_with_master_key(
        master_key_path: impl Into<String>,
        value: &str,
    ) -> Result<Self, ServerConfigError> {
        let cipher = Cipher::new(FileKeySource::new(master_key_path));
        Self::encrypt_with_cipher(&cipher, value)
    }

    pub fn decrypt_with_cipher(&self, cipher: &Cipher) -> Result<String, ServerConfigError> {
        let nonce = STANDARD.decode(&self.nonce).map_err(|source| {
            ServerConfigError::InvalidSecretEncoding {
                field: "nonce",
                source,
            }
        })?;
        let ciphertext = STANDARD.decode(&self.encrypted).map_err(|source| {
            ServerConfigError::InvalidSecretEncoding {
                field: "encrypted",
                source,
            }
        })?;
        Ok(cipher
            .decrypt(&nonce, &ciphertext)?
            .expose_secret()
            .to_string())
    }
}

#[derive(Debug, Error)]
pub enum ServerConfigError {
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse TOML config: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid bind address `{value}`: {source}")]
    InvalidBindAddr {
        value: String,
        #[source]
        source: std::net::AddrParseError,
    },
    #[error("invalid secret {field} base64: {source}")]
    InvalidSecretEncoding {
        field: &'static str,
        #[source]
        source: base64::DecodeError,
    },
    #[error("{0}")]
    Crypto(#[from] CryptoError),
}

#[derive(Debug, Deserialize)]
struct TomlConfig {
    server: Option<TomlServer>,
    database: Option<TomlDatabase>,
    trace: Option<TomlTrace>,
    crypto: Option<TomlCrypto>,
    auth: Option<TomlAuth>,
    logging: Option<TomlLogging>,
}

#[derive(Debug, Deserialize)]
struct TomlServer {
    bind_addr: Option<String>,
    web_dist_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlDatabase {
    url: Option<SecretConfigValue>,
}

#[derive(Debug, Deserialize)]
struct TomlTrace {
    dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlCrypto {
    master_key_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TomlAuth {
    dev_enabled: Option<bool>,
    oauth: Option<TomlOAuth>,
}

#[derive(Debug, Deserialize)]
struct TomlOAuth {
    enabled: Option<bool>,
    client_id: Option<String>,
    client_secret: Option<SecretConfigValue>,
    base_url: Option<String>,
    authorize_url: Option<String>,
    token_url: Option<String>,
    userinfo_url: Option<String>,
    logout_url: Option<String>,
    redirect_uri: Option<String>,
    scope: Option<String>,
    cookie_secure: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TomlLogging {
    level: Option<String>,
    file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum SecretConfigValue {
    Plain(String),
    Encrypted(EncryptedConfigValue),
}

impl ServerConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ServerConfigError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| ServerConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml_str(&content)
    }

    pub fn from_toml_str(input: &str) -> Result<Self, ServerConfigError> {
        let raw: TomlConfig = toml::from_str(input)?;
        let master_key_path = raw
            .crypto
            .as_ref()
            .and_then(|crypto| crypto.master_key_path.as_deref())
            .map(expand_home_path)
            .transpose()?
            .unwrap_or_else(default_master_key_path);
        let cipher = Cipher::new(FileKeySource::new(master_key_path.display().to_string()));
        let server = raw.server.as_ref();
        let database = raw.database.as_ref();
        let trace = raw.trace.as_ref();
        let auth = raw.auth.as_ref();
        let oauth = auth.and_then(|auth| auth.oauth.as_ref());
        let logging = raw.logging.as_ref();

        Self::from_values(ConfigValues {
            bind_addr: server.and_then(|server| server.bind_addr.as_deref()),
            web_dist_dir: server.and_then(|server| server.web_dist_dir.as_deref()),
            database_url: database
                .and_then(|database| database.url.as_ref())
                .map(|value| value.resolve(&cipher))
                .transpose()?
                .as_deref(),
            trace_dir: trace.and_then(|trace| trace.dir.as_deref()),
            master_key_path: Some(master_key_path.as_path()),
            dev_auth_enabled: auth.and_then(|auth| auth.dev_enabled),
            oauth_enabled: oauth.and_then(|oauth| oauth.enabled),
            oauth_client_id: oauth.and_then(|oauth| oauth.client_id.as_deref()),
            oauth_client_secret: oauth
                .and_then(|oauth| oauth.client_secret.as_ref())
                .map(|value| value.resolve(&cipher))
                .transpose()?
                .as_deref(),
            oauth_base_url: oauth.and_then(|oauth| oauth.base_url.as_deref()),
            oauth_authorize_url: oauth.and_then(|oauth| oauth.authorize_url.as_deref()),
            oauth_token_url: oauth.and_then(|oauth| oauth.token_url.as_deref()),
            oauth_userinfo_url: oauth.and_then(|oauth| oauth.userinfo_url.as_deref()),
            oauth_logout_url: oauth.and_then(|oauth| oauth.logout_url.as_deref()),
            oauth_redirect_uri: oauth.and_then(|oauth| oauth.redirect_uri.as_deref()),
            oauth_scope: oauth.and_then(|oauth| oauth.scope.as_deref()),
            oauth_cookie_secure: oauth.and_then(|oauth| oauth.cookie_secure),
            logging_level: logging.and_then(|logging| logging.level.as_deref()),
            logging_file_path: logging.and_then(|logging| logging.file_path.as_deref()),
        })
    }

    pub fn from_env() -> Result<Self, ServerConfigError> {
        tracing::warn!("environment based argus-server startup is deprecated; use --config");
        Self::from_env_values(
            std::env::var("ARGUS_SERVER_ADDR").ok().as_deref(),
            std::env::var("ARGUS_WEB_DIST_DIR").ok().as_deref(),
            std::env::var("DATABASE_URL").ok().as_deref(),
            std::env::var("TRACE_DIR").ok().as_deref(),
            std::env::var("ARGUSCLAW_MASTER_KEY_PATH").ok().as_deref(),
            std::env::var("ARGUS_DEV_AUTH_ENABLED").ok().as_deref(),
            std::env::var("ARGUS_OAUTH_ENABLED").ok().as_deref(),
            std::env::var("ARGUS_OAUTH_CLIENT_ID").ok().as_deref(),
            std::env::var("ARGUS_OAUTH_CLIENT_SECRET").ok().as_deref(),
            std::env::var("RUST_LOG").ok().as_deref(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_env_values(
        bind_addr: Option<&str>,
        web_dist_dir: Option<&str>,
        database_url: Option<&str>,
        trace_dir: Option<&str>,
        master_key_path: Option<&str>,
        dev_auth_enabled: Option<&str>,
        oauth_enabled: Option<&str>,
        oauth_client_id: Option<&str>,
        oauth_client_secret: Option<&str>,
        logging_level: Option<&str>,
    ) -> Result<Self, ServerConfigError> {
        Self::from_values(ConfigValues {
            bind_addr,
            web_dist_dir,
            database_url,
            trace_dir,
            master_key_path: master_key_path.map(Path::new),
            dev_auth_enabled: dev_auth_enabled.map(parse_bool),
            oauth_enabled: oauth_enabled.map(parse_bool),
            oauth_client_id,
            oauth_client_secret,
            oauth_base_url: std::env::var("ARGUS_OAUTH_BASE_URL").ok().as_deref(),
            oauth_authorize_url: std::env::var("ARGUS_OAUTH_AUTHORIZE_URL").ok().as_deref(),
            oauth_token_url: std::env::var("ARGUS_OAUTH_TOKEN_URL").ok().as_deref(),
            oauth_userinfo_url: std::env::var("ARGUS_OAUTH_USERINFO_URL").ok().as_deref(),
            oauth_logout_url: std::env::var("ARGUS_OAUTH_LOGOUT_URL").ok().as_deref(),
            oauth_redirect_uri: std::env::var("ARGUS_OAUTH_REDIRECT_URI").ok().as_deref(),
            oauth_scope: std::env::var("ARGUS_OAUTH_SCOPE").ok().as_deref(),
            oauth_cookie_secure: std::env::var("ARGUS_OAUTH_COOKIE_SECURE")
                .ok()
                .as_deref()
                .map(parse_bool),
            logging_level,
            logging_file_path: None,
        })
    }

    pub fn load(config_path: Option<&Path>) -> Result<LoadedServerConfig, ServerConfigError> {
        if let Some(path) = config_path {
            return Ok(LoadedServerConfig {
                config: Self::from_file(path)?,
                source: ConfigSource::File(path.to_path_buf()),
            });
        }

        for candidate in [Path::new(DEPLOY_CONFIG_PATH), Path::new(LOCAL_CONFIG_PATH)] {
            if candidate.exists() {
                return Ok(LoadedServerConfig {
                    config: Self::from_file(candidate)?,
                    source: ConfigSource::File(candidate.to_path_buf()),
                });
            }
        }

        Ok(LoadedServerConfig {
            config: Self::from_env()?,
            source: ConfigSource::Environment,
        })
    }

    fn from_values(values: ConfigValues<'_>) -> Result<Self, ServerConfigError> {
        let bind_addr_raw = values.bind_addr.unwrap_or(DEFAULT_BIND_ADDR);
        let bind_addr =
            bind_addr_raw
                .parse()
                .map_err(|source| ServerConfigError::InvalidBindAddr {
                    value: bind_addr_raw.to_string(),
                    source,
                })?;
        let master_key_path = values
            .master_key_path
            .map(|path| expand_home_path(&path.display().to_string()))
            .transpose()?
            .unwrap_or_else(default_master_key_path);
        Ok(Self {
            bind_addr,
            web_dist_dir: values.web_dist_dir.map(PathBuf::from),
            database_url: values.database_url.unwrap_or_default().to_string(),
            trace_dir: values
                .trace_dir
                .map(PathBuf::from)
                .unwrap_or_else(default_trace_dir),
            master_key_path,
            auth: AuthSettings {
                dev_enabled: values.dev_auth_enabled.unwrap_or(false),
                oauth: OAuthSettings {
                    enabled: values.oauth_enabled.unwrap_or(false),
                    client_id: values.oauth_client_id.map(str::to_string),
                    client_secret: values.oauth_client_secret.map(str::to_string),
                    base_url: values.oauth_base_url.map(str::to_string),
                    authorize_url: values.oauth_authorize_url.map(str::to_string),
                    token_url: values.oauth_token_url.map(str::to_string),
                    userinfo_url: values.oauth_userinfo_url.map(str::to_string),
                    logout_url: values.oauth_logout_url.map(str::to_string),
                    redirect_uri: values.oauth_redirect_uri.map(str::to_string),
                    scope: values.oauth_scope.map(str::to_string),
                    cookie_secure: values.oauth_cookie_secure,
                },
            },
            logging: LoggingConfig {
                level: values.logging_level.map(str::to_string),
                file_path: values.logging_file_path.map(PathBuf::from),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedServerConfig {
    pub config: ServerConfig,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    File(PathBuf),
    Environment,
}

impl fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigSource::File(path) => write!(f, "{}", path.display()),
            ConfigSource::Environment => f.write_str("environment"),
        }
    }
}

struct ConfigValues<'a> {
    bind_addr: Option<&'a str>,
    web_dist_dir: Option<&'a str>,
    database_url: Option<&'a str>,
    trace_dir: Option<&'a str>,
    master_key_path: Option<&'a Path>,
    dev_auth_enabled: Option<bool>,
    oauth_enabled: Option<bool>,
    oauth_client_id: Option<&'a str>,
    oauth_client_secret: Option<&'a str>,
    oauth_base_url: Option<&'a str>,
    oauth_authorize_url: Option<&'a str>,
    oauth_token_url: Option<&'a str>,
    oauth_userinfo_url: Option<&'a str>,
    oauth_logout_url: Option<&'a str>,
    oauth_redirect_uri: Option<&'a str>,
    oauth_scope: Option<&'a str>,
    oauth_cookie_secure: Option<bool>,
    logging_level: Option<&'a str>,
    logging_file_path: Option<&'a str>,
}

impl SecretConfigValue {
    fn resolve(&self, cipher: &Cipher) -> Result<String, ServerConfigError> {
        match self {
            Self::Plain(value) => Ok(value.clone()),
            Self::Encrypted(value) => value.decrypt_with_cipher(cipher),
        }
    }
}

pub fn default_trace_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".arguswing")
        .join("traces")
}

fn default_master_key_path() -> PathBuf {
    expand_home_path(DEFAULT_MASTER_KEY_PATH)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_MASTER_KEY_PATH))
}

fn expand_home_path(path: &str) -> Result<PathBuf, ServerConfigError> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let home_dir = dirs::home_dir().ok_or_else(|| {
            ServerConfigError::Crypto(CryptoError::SecretKeyMaterialUnavailable {
                reason: "failed to resolve home directory".to_string(),
            })
        })?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(PathBuf::from(path))
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}
