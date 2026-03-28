//! TokenLLMProvider - LLM provider with automatic auth header injection.

use std::sync::Arc;
use std::time::{Duration, Instant};

use argus_crypto::Cipher;
use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, LlmError, LlmEventStream, LlmProvider,
};
use argus_repository::traits::AccountRepository;
use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::RwLock;

use super::error::AuthError;

/// Token endpoint configuration for token-based auth.
#[derive(Clone)]
pub struct TokenConfig {
    pub token_url: String,
    pub header_name: String,
    pub header_prefix: String,
    pub refresh_interval: Duration,
}

impl TokenConfig {
    #[must_use]
    pub fn new(token_url: String, header_name: String, header_prefix: String) -> Self {
        Self {
            token_url,
            header_name,
            header_prefix,
            refresh_interval: Duration::from_secs(300),
        }
    }

    #[must_use]
    pub fn with_refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = interval;
        self
    }
}

/// Holds auth dependencies needed to construct token-wrapped LLM providers.
#[derive(Clone)]
pub struct TokenContext {
    pub account_repo: Arc<dyn AccountRepository>,
    pub cipher: Arc<Cipher>,
    pub config: TokenConfig,
}

#[async_trait]
pub trait TokenSource: Send + Sync {
    async fn fetch_token(&self, username: &str, password: &str) -> Result<String, AuthError>;
    fn header_name(&self) -> &str;
    fn header_prefix(&self) -> &str;
}

pub struct SimpleTokenSource {
    token_url: String,
    header_name: String,
    header_prefix: String,
}

impl SimpleTokenSource {
    #[must_use]
    pub fn new(token_url: String, header_name: String, header_prefix: String) -> Self {
        Self {
            token_url,
            header_name,
            header_prefix,
        }
    }
}

#[async_trait]
impl TokenSource for SimpleTokenSource {
    async fn fetch_token(&self, username: &str, password: &str) -> Result<String, AuthError> {
        let client = reqwest::Client::new();
        let response = client
            .post(&self.token_url)
            .json(&serde_json::json!({
                "username": username,
                "password": password
            }))
            .send()
            .await
            .map_err(|e: reqwest::Error| AuthError::TokenFetchFailed {
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(AuthError::TokenFetchFailed {
                reason: format!("HTTP {}", response.status()),
            });
        }

        let body: serde_json::Value =
            response
                .json()
                .await
                .map_err(|e: reqwest::Error| AuthError::TokenFetchFailed {
                    reason: format!("Failed to parse response: {e}"),
                })?;

        body.get("token")
            .and_then(|t: &serde_json::Value| t.as_str())
            .map(String::from)
            .ok_or_else(|| AuthError::TokenFetchFailed {
                reason: "Response missing 'token' field".to_string(),
            })
    }

    fn header_name(&self) -> &str {
        &self.header_name
    }

    fn header_prefix(&self) -> &str {
        &self.header_prefix
    }
}

/// TokenSource that fetches credentials from the accounts table via AccountRepository.
/// Token URL, header_name, header_prefix are hardcoded.
pub struct AccountTokenSource {
    repo: Arc<dyn AccountRepository>,
    cipher: Arc<Cipher>,
    header_name: String,
    header_prefix: String,
    token_url: &'static str,
}

impl AccountTokenSource {
    #[must_use]
    pub fn new(repo: Arc<dyn AccountRepository>, cipher: Arc<Cipher>) -> Self {
        Self {
            repo,
            cipher,
            header_name: "Authorization".to_string(),
            header_prefix: "Bearer ".to_string(),
            token_url: "https://auth.example.com/token",
        }
    }
}

#[async_trait]
impl TokenSource for AccountTokenSource {
    async fn fetch_token(&self, _username: &str, _password: &str) -> Result<String, AuthError> {
        let creds = self
            .repo
            .get_credentials()
            .await
            .map_err(|e| AuthError::DatabaseError {
                reason: e.to_string(),
            })?
            .ok_or(AuthError::TokenNotAvailable)?;

        let decrypted = self
            .cipher
            .decrypt(&creds.nonce, &creds.ciphertext)
            .map_err(|e| AuthError::DecryptionFailed {
                reason: e.to_string(),
            })?;

        let client = reqwest::Client::new();
        let response = client
            .post(self.token_url)
            .json(&serde_json::json!({
                "username": creds.username,
                "password": decrypted.expose_secret()
            }))
            .send()
            .await
            .map_err(|e: reqwest::Error| AuthError::TokenFetchFailed {
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(AuthError::TokenFetchFailed {
                reason: format!("HTTP {}", response.status()),
            });
        }

        let body: serde_json::Value =
            response
                .json()
                .await
                .map_err(|e: reqwest::Error| AuthError::TokenFetchFailed {
                    reason: format!("Failed to parse response: {e}"),
                })?;

        body.get("token")
            .and_then(|t: &serde_json::Value| t.as_str())
            .map(String::from)
            .ok_or_else(|| AuthError::TokenFetchFailed {
                reason: "Response missing 'token' field".to_string(),
            })
    }

    fn header_name(&self) -> &str {
        &self.header_name
    }

    fn header_prefix(&self) -> &str {
        &self.header_prefix
    }
}

struct TokenCache {
    token: Option<String>,
    last_refresh: Option<Instant>,
    refresh_interval: Duration,
}

impl TokenCache {
    fn new(refresh_interval: Duration) -> Self {
        Self {
            token: None,
            last_refresh: None,
            refresh_interval,
        }
    }

    fn needs_refresh(&self) -> bool {
        match (&self.token, self.last_refresh) {
            (None, _) => true,
            (_, None) => true,
            (Some(_), Some(last)) => last.elapsed() >= self.refresh_interval,
        }
    }

    fn update(&mut self, token: String) {
        self.token = Some(token);
        self.last_refresh = Some(Instant::now());
    }
}

pub struct TokenLLMProvider<T> {
    inner: T,
    cache: Arc<RwLock<TokenCache>>,
    provider: Arc<dyn TokenSource>,
    username: String,
    password: String,
}

impl<T> TokenLLMProvider<T> {
    #[must_use]
    pub fn new(
        inner: T,
        provider: Arc<dyn TokenSource>,
        username: String,
        password: String,
        refresh_interval: Duration,
    ) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(TokenCache::new(refresh_interval))),
            provider,
            username,
            password,
        }
    }
}

#[async_trait]
impl<T: LlmProvider> LlmProvider for TokenLLMProvider<T> {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.inner.cost_per_token()
    }

    fn capabilities(&self) -> argus_protocol::llm::ProviderCapabilities {
        self.inner.capabilities()
    }

    async fn complete(
        &self,
        mut request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let extra_header = self
            .get_auth_header()
            .await
            .map_err(|e| LlmError::AuthFailed {
                provider: self.inner.active_model_name(),
                reason: e.to_string(),
            })?;

        request.extra_headers.push(extra_header);
        self.inner.complete(request).await
    }

    async fn stream_complete(
        &self,
        mut request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let extra_header = self
            .get_auth_header()
            .await
            .map_err(|e| LlmError::AuthFailed {
                provider: self.inner.active_model_name(),
                reason: e.to_string(),
            })?;

        request.extra_headers.push(extra_header);
        self.inner.stream_complete(request).await
    }
}

impl<T> TokenLLMProvider<T> {
    async fn get_auth_header(&self) -> Result<(String, String), AuthError> {
        let mut cache = self.cache.write().await;
        if cache.needs_refresh() {
            let token = self
                .provider
                .fetch_token(&self.username, &self.password)
                .await?;
            cache.update(token);
        }

        let token = cache.token.as_ref().ok_or(AuthError::TokenNotAvailable)?;
        Ok((
            self.provider.header_name().to_string(),
            format!("{}{}", self.provider.header_prefix(), token),
        ))
    }
}
