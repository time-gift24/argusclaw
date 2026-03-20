# Authentication & Credentials Design

**Date**: 2026-03-19
**Status**: Draft

## Overview

This document describes the authentication and credentials system for ArgusWing desktop application.

## Requirements Summary

| Requirement | Choice |
|-------------|--------|
| Application Mode | Single-user local application |
| Credential Security | Master key encryption (reuse LLM provider key file) |
| First-time Setup | Guided setup wizard (welcome page) |
| External Credentials | Global credential pool (stored by name) |
| LLM Token | TokenLLMProvider with auto-refresh |
| Encryption Module | Extract to `argus-crypto` crate |

## Architecture

```
desktop (Tauri)
├── React Frontend
│   ├── use-auth-store.ts (state management)
│   └── login-dialog.tsx (guided setup)
├── Tauri Commands (commands.rs)
│   ├── auth: get_current_user, has_any_user, setup_account
│   └── credentials: list, add, update, delete
├── argus-crypto (NEW)
│   └── ApiKeyCipher, KeyMaterialSource, EncryptedSecret
└── argus-auth (NEW)
    ├── AccountManager: setup/login/logout
    ├── CredentialStore: external credentials
    └── TokenLLMProvider: LLM with auth header
```

---

## Section 1: Crypto Module (argus-crypto)

**Goal**: Extract encryption logic from `argus-llm/src/secret.rs` to a standalone crate.

### File Structure

```
crates/argus-crypto/
├── Cargo.toml
├── src/
│   ├── lib.rs           # Public exports
│   ├── cipher.rs        # ApiKeyCipher, EncryptedSecret
│   ├── key_source.rs    # KeyMaterialSource trait + implementations
│   └── error.rs         # CryptoError
└── tests/
```

### API Design

```rust
// lib.rs
pub use cipher::{ApiKeyCipher, EncryptedSecret};
pub use key_source::{
    FileKeyMaterialSource,    // Master key file (~/.arguswing/master.key)
    HostMacAddressKeySource,  // Host MAC address
    StaticKeySource,         // Testing only
};
pub use error::CryptoError;

// Usage
let cipher = ApiKeyCipher::new(FileKeyMaterialSource::from_env_or_default());
let encrypted = cipher.encrypt("sensitive-data")?;
let decrypted = cipher.decrypt(&encrypted.nonce, &encrypted.ciphertext)?;
```

### Dependencies

```toml
[dependencies]
ring = "0.17"           # AES-256-GCM, HKDF
mac_address = "1.1"    # Get MAC address
dirs = "5"             # Home dir resolution
thiserror = "1"        # Error types
serde = { version = "1", features = ["derive"] }
```

### Migration Plan

1. Create `crates/argus-crypto`
2. Copy `argus-llm/src/secret.rs` content
3. Update imports in `argus-llm` to use `argus-crypto`
4. Update `argus-repository` if needed

---

## Section 2: Auth Module (argus-auth)

**Goal**: Implement account management and TokenLLMProvider.

### File Structure

```
crates/argus-auth/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── account.rs       # AccountManager: setup/login/logout
│   ├── credential.rs    # CredentialStore: external credentials
│   ├── token.rs         # TokenLLMProvider
│   └── error.rs         # AuthError
└── tests/
```

### AccountManager API

```rust
pub struct AccountManager {
    pool: Arc<SqlitePool>,
    cipher: Arc<ApiKeyCipher>,
}

impl AccountManager {
    /// Check if account exists
    pub async fn has_account(&self) -> Result<bool, AuthError>;

    /// Create first account (only works when no account exists)
    pub async fn setup_account(&self, username: &str, password: &str) -> Result<(), AuthError>;

    /// Login verification
    pub async fn login(&self, username: &str, password: &str) -> Result<bool, AuthError>;

    /// Logout (no-op for single-user local app; reserved for future multi-session support)
    pub async fn logout(&self) -> Result<(), AuthError> {
        Ok(())  // No session state to clear
    }

    /// Get current user info
    pub async fn get_current_user(&self) -> Result<Option<UserInfo>, AuthError>;
}
```

> **Note**: `AccountManager` takes `Arc<SqlitePool>` directly instead of `Arc<ArgusSqlite>` to avoid coupling with `argus-repository`'s internal types. `argus-auth` depends only on `argus-crypto` and `sqlx`.

### Account Database Schema

```sql
CREATE TABLE accounts (
    id          INTEGER PRIMARY KEY CHECK (id = 1),  -- Single account, always id=1
    username    TEXT NOT NULL,
    password    BLOB NOT NULL,  -- Encrypted with master key
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### Password Storage

- Passwords are encrypted with the master key (same key file as LLM API keys)
- Uses `ApiKeyCipher::encrypt()` / `decrypt()` from argus-crypto
- No hashing - this is for external credential storage, not application authentication

### User Info

```rust
pub struct UserInfo {
    pub username: String,
}
```

### Password Policy

- Minimal: any non-empty password accepted
- No strength validation required

---

## Section 3: Credential Storage

**Goal**: Store external system credentials (username/password) that agents/tools can reference by name.

### Database Schema

```sql
CREATE TABLE credentials (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    username    BLOB NOT NULL,
    password    BLOB NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### API Design

```rust
// === CredentialRecord ===
pub struct CredentialRecord {
    pub id: i64,
    pub name: String,
    pub username: String,  // Decrypted
    pub password: String,  // Decrypted
}

// === CredentialSummary ===
pub struct CredentialSummary {
    pub id: i64,
    pub name: String,
}

// === CredentialStore ===
impl CredentialStore {
    pub async fn list(&self) -> Result<Vec<CredentialSummary>, AuthError>;
    pub async fn get(&self, id: i64) -> Result<Option<CredentialRecord>, AuthError>;
    pub async fn get_by_name(&self, name: &str) -> Result<Option<CredentialRecord>, AuthError>;
    pub async fn add(&self, name: &str, username: &str, password: &str) -> Result<i64, AuthError>;
    pub async fn update(&self, id: i64, username: Option<&str>, password: Option<&str>) -> Result<(), AuthError>;
    pub async fn delete(&self, id: i64) -> Result<bool, AuthError>;
}
```

### Tauri Commands

```rust
// === Account Commands ===
#[tauri::command]
pub async fn get_current_user(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Option<UserInfo>, String>;

#[tauri::command]
pub async fn has_any_user(wing: State<'_, Arc<ArgusWing>>) -> Result<bool, String>;

#[tauri::command]
pub async fn setup_account(
    wing: State<'_, Arc<ArgusWing>>,
    username: String,
    password: String,
) -> Result<(), String>;

#[tauri::command]
pub async fn login(
    wing: State<'_, Arc<ArgusWing>>,
    username: String,
    password: String,
) -> Result<bool, String>;

#[tauri::command]
pub async fn logout(wing: State<'_, Arc<ArgusWing>>) -> Result<(), String>;

// === Credential Commands ===
#[tauri::command]
pub async fn list_credentials(wing: State<'_, Arc<ArgusWing>>) -> Result<Vec<CredentialSummary>, String>;

#[tauri::command]
pub async fn get_credential(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<Option<CredentialRecord>, String>;

#[tauri::command]
pub async fn add_credential(
    wing: State<'_, Arc<ArgusWing>>,
    name: String,
    username: String,
    password: String,
) -> Result<i64, String>;

#[tauri::command]
pub async fn update_credential(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
    username: Option<String>,
    password: Option<String>,
) -> Result<(), String>;

#[tauri::command]
pub async fn delete_credential(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<bool, String>;
```

---

## Section 4: TokenLLMProvider

**Goal**: Wrap existing `LlmProvider`, auto-inject auth header from cached token with periodic refresh.

### Design

```rust
// === TokenSource trait ===
// Note: fetch_token is sync to keep the trait simple. Implementations should use
// blocking HTTP (e.g., reqwest blocking client) internally.
pub trait TokenSource: Send + Sync {
    fn fetch_token(&self, username: &str, password: &str) -> Result<String, AuthError>;
    fn header_name(&self) -> &str;
    fn header_prefix(&self) -> &str;
}

// === SimpleTokenSource ===
pub struct SimpleTokenSource {
    token_url: String,
    header_name: String,
    header_prefix: String,
}

impl TokenSource for SimpleTokenSource {
    fn fetch_token(&self, username: &str, password: &str) -> Result<String, AuthError> {
        // Use reqwest blocking client to POST username/password to token_url
        // Return the token from response body (implementation detail)
    }
}

// === TokenCache ===
struct TokenCache {
    token: Option<String>,
    expires_at: Option<Instant>,
    refresh_interval: Duration,
}

impl TokenCache {
    fn needs_refresh(&self) -> bool;
    fn update(&mut self, token: String);
}

// === TokenLLMProvider<T> ===
pub struct TokenLLMProvider<T> {
    inner: T,
    cache: Arc<tokio::sync::RwLock<TokenCache>>,
    provider: Arc<dyn TokenSource>,
    username: String,
    password: String,
}

impl<T> TokenLLMProvider<T> {
    pub fn new(
        inner: T,
        provider: Arc<dyn TokenSource>,
        username: String,
        password: String,
        refresh_interval: Duration,
    ) -> Self {
        Self {
            inner,
            cache: Arc::new(tokio::sync::RwLock::new(TokenCache::new(refresh_interval))),
            provider,
            username,
            password,
        }
    }
}

impl<T: LlmProvider> LlmProvider for TokenLLMProvider<T> {
    async fn complete(&self, mut request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let header = self.get_auth_header().await?;
        request.extra_headers.push(header);
        self.inner.complete(request).await
    }

    async fn stream_complete(&self, mut request: CompletionRequest) -> Result<LlmEventStream, LlmError> {
        let header = self.get_auth_header().await?;
        request.extra_headers.push(header);
        self.inner.stream_complete(request).await
    }
}

impl<T> TokenLLMProvider<T> {
    /// Get auth header (auto-refresh if needed).
    /// Header type is `http::Header` from the `http` crate.
    async fn get_auth_header(&self) -> Result<http::Header, AuthError> {
        let mut cache = self.cache.write().await;
        if cache.needs_refresh() {
            let token = self.provider.fetch_token(&self.username, &self.password)?;
            cache.update(token);
        }
        let token = cache.token.as_ref().ok_or(AuthError::TokenNotAvailable)?;
        Ok(http::Header::new(
            self.provider.header_name(),
            format!("{}{}", self.provider.header_prefix(), token),
        ))
    }
}
```

### Usage

`TokenLLMProvider` is used internally within `ArgusWing::init()` to wrap providers that require auth. Example:

```rust
// In ArgusWing::init() - wrapping a provider that needs auth
let base_provider = provider_manager.get_provider(provider_id)?;
let auth_provider = TokenLLMProvider::new(
    base_provider,
    token_source,
    "my_user".to_string(),
    "my_pass".to_string(),
    Duration::from_secs(300),  // 5 minute refresh
);

// Store wrapped provider for later use
// (actual storage mechanism is implementation detail)
```

> **Note**: `TokenLLMProvider` is constructed inside `ArgusWing`, not exposed in its public API. External code continues using `get_provider(id)` to get `Arc<dyn LlmProvider>`.

### No Database Table

- Token provider configuration is in-memory only
- No persistence required

---

## Implementation Order

1. **argus-crypto**: Extract from argus-llm
2. **argus-auth**: Create crate structure
3. **AccountManager**: Basic account CRUD
4. **CredentialStore**: Credential storage
5. **TokenLLMProvider**: Auth header wrapper
6. **Frontend integration**: Connect Tauri commands

---

## Section 5: ArgusWing Integration

### Initialization

`ArgusWing` is extended to include auth components:

```rust
// In argus-wing/src/lib.rs
pub struct ArgusWing {
    // ... existing fields (pool, llm_repository, etc.)

    // NEW: Auth components
    pub account_manager: Arc<AccountManager>,
    pub credential_store: Arc<CredentialStore>,
}
```

Updated `ArgusWing::init()`:

```rust
impl ArgusWing {
    pub async fn init() -> Result<Arc<Self>> {
        let pool = SqlitePool::connect(&db_path).await?;

        // Initialize cipher (reuses existing master.key)
        let cipher = ApiKeyCipher::new(FileKeyMaterialSource::from_env_or_default());

        // NEW: Create auth components
        let account_manager = Arc::new(AccountManager::new(
            Arc::new(pool.clone()),
            Arc::new(cipher.clone()),
        ));
        let credential_store = Arc::new(CredentialStore::new(
            Arc::new(pool.clone()),
            Arc::new(cipher),
        ));

        // ... existing initialization ...

        Ok(Arc::new(Self {
            // ... existing fields ...
            account_manager,
            credential_store,
        }))
    }
}
```

### Implementation Notes

- **`ArgusSqlite` is internal to `argus-repository`** — `argus-auth` uses `Arc<SqlitePool>` directly
- **Shared `SqlitePool`**: Both auth and existing managers share the same database pool
- **`ApiKeyCipher` instance**: Reused from existing LLM provider encryption (single master.key)
- **`AccountManager` and `CredentialStore`**: Both constructed in `ArgusWing::init()` with same pool + cipher

### Master Key Lifecycle

The master key file (`~/.arguswing/master.key`) is managed by argus-crypto:

| Event | Behavior |
|-------|----------|
| File missing | Auto-create with 32 random bytes |
| File empty | Auto-regenerate with 32 random bytes |
| File invalid length | Error (cannot recover) |
| File valid | Load and use as-is |

Key file permissions:
- **macOS/Linux**: `chmod 600` (owner read/write only)
- **Windows**: Default filesystem permissions

---

## Open Questions

None. All requirements have been clarified.
