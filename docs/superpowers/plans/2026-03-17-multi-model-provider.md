# Multi-Model LLM Provider Support Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable a single LLM Provider to support multiple configurable models with user-selected default and per-session model override.

**Architecture:** Replace single `model` field with `models` (JSON array) and `default_model` fields in database. Update types through all layers (domain → repository → manager → Tauri commands → frontend). Add model selector UI in provider form and test dialog.

**Tech Stack:** Rust (sqlx, thiserror, serde), TypeScript (React, shadcn/ui), SQLite

---

## File Structure

### Backend (Rust)

| File | Change | Purpose |
|------|--------|---------|
| `crates/claw/migrations/YYYYMMDDHHMMSS_multi_model_provider.sql` | Create | Migration to add models/default_model fields |
| `crates/claw/src/db/llm.rs` | Modify | Update LlmProviderRecord, LlmProviderSummary types |
| `crates/claw/src/db/sqlite/llm.rs` | Modify | Update SQL queries and field mapping |
| `crates/claw/src/error.rs` | Modify | Add ModelNotAvailable error variant |
| `crates/claw/src/llm/manager.rs` | Modify | Add model parameter to get/test methods |
| `crates/claw/src/claw.rs` | Modify | Update AppContext methods with model params |

### Frontend (TypeScript)

| File | Change | Purpose |
|------|--------|---------|
| `crates/desktop/src-tauri/src/commands.rs` | Modify | Update Tauri command signatures |
| `crates/desktop/lib/tauri.ts` | Modify | Update TypeScript types and API functions |
| `crates/desktop/components/settings/provider-form-dialog.tsx` | Modify | Replace model input with tag list + default selector |
| `crates/desktop/components/settings/provider-test-dialog.tsx` | Modify | Add model selector dropdown |
| `crates/desktop/app/settings/providers/page.tsx` | Modify | Update test connection calls with model param |

---

## Chunk 1: Database Migration

### Task 1: Create Migration File

**Files:**
- Create: `crates/claw/migrations/20260317000000_multi_model_provider.sql`

- [ ] **Step 1: Verify migration timestamp will be correct**

Check existing migrations to ensure new migration timestamp will be greater:

```bash
ls -la crates/claw/migrations/
```

Expected: Latest migration is `20260312050414_create_users_table.sql` or earlier. New migration will be `20260317*` which is greater.

- [ ] **Step 2: Create migration file**

Create migration using sqlx-cli:

```bash
cd crates/claw && sqlx migrate add multi_model_provider
```

- [ ] **Step 3: Write migration SQL**

```sql
-- Migration: Multi-model provider support
-- Replaces single 'model' field with 'models' array and 'default_model'

-- Step 1: Create new table with updated schema
CREATE TABLE IF NOT EXISTS llm_providers_new (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    models TEXT NOT NULL DEFAULT '[]',
    default_model TEXT NOT NULL,
    encrypted_api_key BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    extra_headers TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Step 2: Migrate existing data
INSERT INTO llm_providers_new (
    id, kind, display_name, base_url, models, default_model,
    encrypted_api_key, api_key_nonce, extra_headers, is_default,
    created_at, updated_at
)
SELECT
    id, kind, display_name, base_url,
    json_array(model) AS models,
    model AS default_model,
    encrypted_api_key, api_key_nonce, extra_headers, is_default,
    created_at, updated_at
FROM llm_providers;

-- Step 3: Drop old table
DROP TABLE llm_providers;

-- Step 4: Rename new table
ALTER TABLE llm_providers_new RENAME TO llm_providers;

-- Step 5: Recreate indexes
CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_providers_single_default
ON llm_providers (is_default)
WHERE is_default = 1;
```

- [ ] **Step 3: Verify migration compiles**

Run: `cargo check -p claw`
Expected: No errors

- [ ] **Step 4: Commit migration**

```bash
git add crates/claw/migrations/20260317000000_multi_model_provider.sql
git commit -m "$(cat <<'EOF'
feat(claw): add migration for multi-model provider support

Add models (JSON array) and default_model fields to replace single model field.
Migration preserves existing data by converting model -> models[0] + default_model.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 2: Backend Types and Error

### Task 2: Update Error Types

**Files:**
- Modify: `crates/claw/src/error.rs`

- [ ] **Step 1: Write failing test for ModelNotAvailable error**

In `crates/claw/src/error.rs`, add test at end of file:

```rust
#[cfg(test)]
mod tests {
    use super::AgentError;

    #[test]
    fn model_not_available_error_formats_correctly() {
        let error = AgentError::ModelNotAvailable {
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
        };
        let message = error.to_string();
        assert!(message.contains("openai"));
        assert!(message.contains("gpt-5"));
        assert!(message.contains("not available"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p claw model_not_available_error`
Expected: FAIL - variant doesn't exist

- [ ] **Step 3: Add ModelNotAvailable error variant**

In `crates/claw/src/error.rs`, add after `UnsupportedProviderKind`:

```rust
    #[error("model `{model}` is not available on provider `{provider}`")]
    ModelNotAvailable { provider: String, model: String },

    #[error("provider validation failed: {reason}")]
    ProviderValidationFailed { reason: String },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p claw model_not_available_error`
Expected: PASS

- [ ] **Step 5: Commit error types**

```bash
git add crates/claw/src/error.rs
git commit -m "$(cat <<'EOF'
feat(claw): add ModelNotAvailable and ProviderValidationFailed errors

Add error variants for multi-model provider validation:
- ModelNotAvailable: requested model not in provider's models list
- ProviderValidationFailed: general provider validation failures

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

### Task 3: Update Domain Types

**Files:**
- Modify: `crates/claw/src/db/llm.rs`

- [ ] **Step 1: Write failing tests for new types**

Add to end of `crates/claw/src/db/llm.rs`:

```rust
#[cfg(test)]
mod multi_model_tests {
    use super::*;

    #[test]
    fn llm_provider_record_has_models_and_default_model() {
        let record = LlmProviderRecord {
            id: LlmProviderId::new("test"),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        assert_eq!(record.models.len(), 2);
        assert_eq!(record.default_model, "gpt-4.1");
    }

    #[test]
    fn llm_provider_summary_has_models_and_default_model() {
        let summary = LlmProviderSummary {
            id: LlmProviderId::new("test"),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            models: vec!["o3".to_string()],
            default_model: "o3".to_string(),
            is_default: false,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        assert_eq!(summary.models, vec!["o3"]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p claw multi_model`
Expected: FAIL - fields don't exist

- [ ] **Step 3: Update LlmProviderRecord struct**

Replace `model: String` field with `models: Vec<String>` and `default_model: String`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderRecord {
    pub id: LlmProviderId,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: SecretString,
    pub models: Vec<String>,           // Changed from single model
    pub default_model: String,         // New field
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}
```

- [ ] **Step 4: Update LlmProviderSummary struct**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderSummary {
    pub id: LlmProviderId,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub models: Vec<String>,           // Changed from single model
    pub default_model: String,         // New field
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}
```

- [ ] **Step 5: Update From impl for Summary**

```rust
impl From<LlmProviderRecord> for LlmProviderSummary {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id,
            kind: record.kind,
            display_name: record.display_name,
            base_url: record.base_url,
            models: record.models,
            default_model: record.default_model,
            is_default: record.is_default,
            extra_headers: record.extra_headers,
            secret_status: record.secret_status,
        }
    }
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test -p claw multi_model`
Expected: PASS

- [ ] **Step 7: Commit domain types**

```bash
git add crates/claw/src/db/llm.rs
git commit -m "$(cat <<'EOF'
feat(claw): update LlmProviderRecord/Summary for multi-model

Replace single model field with:
- models: Vec<String> - list of available models
- default_model: String - model used when none specified

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 3: SQLite Repository

### Task 4: Update SQLite Implementation

**Files:**
- Modify: `crates/claw/src/db/sqlite/llm.rs`

- [ ] **Step 1: Update SharedProviderFields type alias**

Change from 8-tuple to include models and default_model:

```rust
type SharedProviderFields = (
    LlmProviderId,
    crate::db::llm::LlmProviderKind,
    String,  // display_name
    String,  // base_url
    Vec<String>,  // models
    String,  // default_model
    bool,    // is_default
    HashMap<String, String>,  // extra_headers
    Vec<u8>,  // nonce
    Vec<u8>,  // ciphertext
);
```

- [ ] **Step 2: Update parse_shared_fields function**

Replace model parsing with models/default_model:

```rust
fn parse_shared_fields(row: sqlx::sqlite::SqliteRow) -> Result<SharedProviderFields, DbError> {
    let nonce: Vec<u8> = row
        .try_get("api_key_nonce")
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
    let ciphertext: Vec<u8> =
        row.try_get("encrypted_api_key")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

    let extra_headers_json: String =
        row.try_get("extra_headers")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
    let extra_headers: HashMap<String, String> = serde_json::from_str(&extra_headers_json)
        .map_err(|e| DbError::QueryFailed {
            reason: format!("failed to parse extra_headers: {e}"),
        })?;

    let models_json: String =
        row.try_get("models")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
    let models: Vec<String> = serde_json::from_str(&models_json)
        .map_err(|e| DbError::QueryFailed {
            reason: format!("failed to parse models: {e}"),
        })?;

    Ok((
        LlmProviderId::new(row.try_get::<String, _>("id").map_err(|e| {
            DbError::QueryFailed {
                reason: e.to_string(),
            }
        })?),
        row.try_get::<String, _>("kind")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?
            .parse()?,
        row.try_get("display_name")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
        row.try_get("base_url").map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?,
        models,
        row.try_get("default_model").map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?,
        row.try_get::<i64, _>("is_default")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?
            != 0,
        extra_headers,
        nonce,
        ciphertext,
    ))
}
```

- [ ] **Step 3: Update map_record function**

```rust
fn map_record(&self, row: sqlx::sqlite::SqliteRow) -> Result<LlmProviderRecord, DbError> {
    let (id, kind, display_name, base_url, models, default_model, is_default, extra_headers, nonce, ciphertext) =
        Self::parse_shared_fields(row)?;

    Ok(LlmProviderRecord {
        id,
        kind,
        display_name,
        base_url,
        api_key: self.decrypt_secret(&nonce, &ciphertext)?,
        models,
        default_model,
        is_default,
        extra_headers,
        secret_status: ProviderSecretStatus::Ready,
    })
}
```

- [ ] **Step 4: Update map_summary function**

```rust
fn map_summary(&self, row: sqlx::sqlite::SqliteRow) -> Result<LlmProviderSummary, DbError> {
    let (id, kind, display_name, base_url, models, default_model, is_default, extra_headers, nonce, ciphertext) =
        Self::parse_shared_fields(row)?;
    let secret_status = if self.decrypt_secret(&nonce, &ciphertext).is_ok() {
        ProviderSecretStatus::Ready
    } else {
        ProviderSecretStatus::RequiresReentry
    };

    Ok(LlmProviderSummary {
        id,
        kind,
        display_name,
        base_url,
        models,
        default_model,
        is_default,
        extra_headers,
        secret_status,
    })
}
```

- [ ] **Step 5: Update upsert_provider SQL**

```rust
async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<(), DbError> {
    // Validation
    if record.models.is_empty() {
        return Err(DbError::QueryFailed {
            reason: "At least one model is required".to_string(),
        });
    }
    if !record.models.contains(&record.default_model) {
        return Err(DbError::QueryFailed {
            reason: format!(
                "Default model '{}' must be in models list",
                record.default_model
            ),
        });
    }

    let encrypted_secret = self.write_cipher.encrypt(record.api_key.expose_secret())?;
    let extra_headers_json =
        serde_json::to_string(&record.extra_headers).map_err(|e| DbError::QueryFailed {
            reason: format!("failed to serialize extra_headers: {e}"),
        })?;
    let models_json =
        serde_json::to_string(&record.models).map_err(|e| DbError::QueryFailed {
            reason: format!("failed to serialize models: {e}"),
        })?;

    let mut transaction = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
        reason: e.to_string(),
    })?;

    if record.is_default {
        sqlx::query("update llm_providers set is_default = 0, updated_at = CURRENT_TIMESTAMP where id != ?1 and is_default = 1")
            .bind(record.id.as_ref())
            .execute(&mut *transaction)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
    }

    sqlx::query(
        "insert into llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         on conflict(id) do update set
             kind = excluded.kind,
             display_name = excluded.display_name,
             base_url = excluded.base_url,
             models = excluded.models,
             default_model = excluded.default_model,
             encrypted_api_key = excluded.encrypted_api_key,
             api_key_nonce = excluded.api_key_nonce,
             is_default = excluded.is_default,
             extra_headers = excluded.extra_headers,
             updated_at = CURRENT_TIMESTAMP",
    )
    .bind(record.id.as_ref())
    .bind(record.kind.as_str())
    .bind(&record.display_name)
    .bind(&record.base_url)
    .bind(&models_json)
    .bind(&record.default_model)
    .bind(encrypted_secret.ciphertext)
    .bind(encrypted_secret.nonce)
    .bind(i64::from(record.is_default))
    .bind(&extra_headers_json)
    .execute(&mut *transaction)
    .await
    .map_err(|e| DbError::QueryFailed {
        reason: e.to_string(),
    })?;

    transaction
        .commit()
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

    Ok(())
}
```

- [ ] **Step 6: Update SELECT queries to include new fields**

Update all queries in `get_provider`, `get_provider_summary`, `list_providers`, `get_default_provider`:

```sql
select id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
from llm_providers
...
```

- [ ] **Step 7: Run tests to verify**

Run: `cargo test -p claw db_sqlite_llm`
Expected: All tests pass

- [ ] **Step 8: Commit SQLite changes**

```bash
git add crates/claw/src/db/sqlite/llm.rs
git commit -m "$(cat <<'EOF'
feat(claw): update SQLite repository for multi-model providers

- Parse models JSON array and default_model from database
- Add validation: models must be non-empty, default_model must be in list
- Update all SQL queries for new schema

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 4: LLM Manager and AppContext

### Task 5: Update LLM Manager

**Files:**
- Modify: `crates/claw/src/llm/manager.rs`

- [ ] **Step 1: Add get_provider_with_model method**

Add new method after `get_provider`:

```rust
    /// Get provider configured with a specific model.
    ///
    /// # Errors
    ///
    /// Returns `ModelNotAvailable` if the model is not in the provider's models list.
    pub async fn get_provider_with_model(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_provider(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })?;

        if !record.models.contains(&model.to_string()) {
            return Err(AgentError::ModelNotAvailable {
                provider: id.to_string(),
                model: model.to_string(),
            });
        }

        self.build_provider_with_model(record, model)
    }
```

- [ ] **Step 2: Update existing get_provider to use default_model**

```rust
    pub async fn get_provider(&self, id: &LlmProviderId) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_provider(id)
            .await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })?;

        self.build_provider_with_model(record, &record.default_model)
    }
```

- [ ] **Step 3: Update get_default_provider similarly**

```rust
    pub async fn get_default_provider(&self) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self
            .repository
            .get_default_provider()
            .await?
            .ok_or(AgentError::DefaultProviderNotConfigured)?;

        self.build_provider_with_model(record, &record.default_model)
    }
```

- [ ] **Step 4: Add build_provider_with_model helper**

Replace `build_provider` with:

```rust
    fn build_provider_with_model(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        match record.kind {
            LlmProviderKind::OpenAiCompatible => {
                #[cfg(feature = "openai-compatible")]
                {
                    let mut config = crate::llm::providers::OpenAiCompatibleConfig::new(
                        record.base_url,
                        record.api_key.expose_secret().to_string(),
                        model.to_string(),  // Use specified model
                    );

                    for (name, value) in &record.extra_headers {
                        config = config.with_extra_header(name, value);
                    }

                    let factory_config =
                        crate::llm::providers::OpenAiCompatibleFactoryConfig::new(config);

                    crate::llm::providers::create_openai_compatible_provider(factory_config)
                        .map_err(AgentError::from)
                }

                #[cfg(not(feature = "openai-compatible"))]
                {
                    Err(AgentError::UnsupportedProviderKind {
                        kind: record.kind.to_string(),
                    })
                }
            }
        }
    }
```

- [ ] **Step 5: Update test_provider_connection to accept model parameter**

```rust
    pub async fn test_provider_connection(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        let Some(record) = self.repository.get_provider(id).await? else {
            return Ok(build_provider_test_result(
                id.to_string(),
                model.to_string(),
                String::new(),
                Duration::ZERO,
                ProviderTestStatus::ProviderNotFound,
                AgentError::ProviderNotFound { id: id.to_string() }.to_string(),
            ));
        };

        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();

        if !record.models.contains(&model.to_string()) {
            return Ok(build_provider_test_result(
                provider_id,
                model.to_string(),
                base_url,
                Duration::ZERO,
                ProviderTestStatus::ModelNotAvailable,
                AgentError::ModelNotAvailable {
                    provider: provider_id,
                    model: model.to_string(),
                }
                .to_string(),
            ));
        }

        let provider = match self.build_provider_with_model(record, model) {
            Ok(provider) => provider,
            Err(AgentError::UnsupportedProviderKind { kind }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    AgentError::UnsupportedProviderKind { kind }.to_string(),
                ));
            }
            Err(error) => return Err(error),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }
```

- [ ] **Step 6: Update test_provider_record similarly**

```rust
    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        let provider_id = record.id.to_string();
        let base_url = record.base_url.clone();

        if !record.models.contains(&model.to_string()) {
            return Ok(build_provider_test_result(
                provider_id,
                model.to_string(),
                base_url,
                Duration::ZERO,
                ProviderTestStatus::ModelNotAvailable,
                AgentError::ModelNotAvailable {
                    provider: provider_id,
                    model: model.to_string(),
                }
                .to_string(),
            ));
        }

        let provider = match self.build_provider_with_model(record, model) {
            Ok(provider) => provider,
            Err(AgentError::UnsupportedProviderKind { kind }) => {
                return Ok(build_provider_test_result(
                    provider_id,
                    model.to_string(),
                    base_url,
                    Duration::ZERO,
                    ProviderTestStatus::UnsupportedProviderKind,
                    AgentError::UnsupportedProviderKind { kind }.to_string(),
                ));
            }
            Err(error) => return Err(error),
        };

        Ok(run_provider_connection_test(provider_id, model.to_string(), base_url, provider).await)
    }
```

- [ ] **Step 7: Update run_provider_connection_test signature**

```rust
async fn run_provider_connection_test(
    provider_id: String,
    model: String,
    base_url: String,
    provider: Arc<dyn LlmProvider>,
) -> ProviderTestResult {
    // ... same implementation, model is now a parameter
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test -p claw llm_manager`
Expected: Tests may need updates for new signatures

- [ ] **Step 9: Commit LLM manager changes**

```bash
git add crates/claw/src/llm/manager.rs
git commit -m "$(cat <<'EOF'
feat(claw): add model parameter support to LLM manager

- Add get_provider_with_model for explicit model selection
- Update test_provider_connection/record to accept model param
- build_provider_with_model creates provider with specified model

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

### Task 6: Update AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

- [ ] **Step 1: Add get_provider_with_model method**

```rust
    /// Get provider with a specific model.
    pub async fn get_provider_with_model(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        self.llm_manager.get_provider_with_model(id, model).await
    }
```

- [ ] **Step 2: Update test_provider_connection signature**

```rust
    pub async fn test_provider_connection(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        self.llm_manager.test_provider_connection(id, model).await
    }
```

- [ ] **Step 3: Update test_provider_record signature**

```rust
    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        self.llm_manager.test_provider_record(record, model).await
    }
```

- [ ] **Step 4: Update existing tests that use model field**

Find and update tests in claw.rs that create LlmProviderRecord with `model:` field:

```rust
// Old:
model: "gpt-4.1".to_string(),

// New:
models: vec!["gpt-4.1".to_string()],
default_model: "gpt-4.1".to_string(),
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p claw`
Expected: All tests pass

- [ ] **Step 6: Commit AppContext changes**

```bash
git add crates/claw/src/claw.rs
git commit -m "$(cat <<'EOF'
feat(claw): update AppContext with model parameter support

Add get_provider_with_model and update test methods to accept model.
Update existing tests to use models/default_model fields.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 5: Tauri Commands

### Task 7: Update Tauri Commands

**Files:**
- Modify: `crates/desktop/src-tauri/src/commands.rs`

- [ ] **Step 1: Update ProviderInput struct**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInput {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,           // Changed
    pub default_model: String,         // New
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}
```

- [ ] **Step 2: Update ProviderInput → LlmProviderRecord conversion**

```rust
impl From<ProviderInput> for LlmProviderRecord {
    fn from(input: ProviderInput) -> Self {
        Self {
            id: LlmProviderId::new(input.id),
            kind: input.kind.into(),
            display_name: input.display_name,
            base_url: input.base_url,
            api_key: SecretString::new(input.api_key),
            models: input.models,
            default_model: input.default_model,
            is_default: input.is_default,
            extra_headers: input.extra_headers,
            secret_status: ProviderSecretStatus::Ready,
        }
    }
}
```

- [ ] **Step 3: Update ProviderSummary struct**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub models: Vec<String>,           // Changed
    pub default_model: String,         // New
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}
```

- [ ] **Step 4: Update LlmProviderSummary → ProviderSummary conversion**

```rust
impl From<LlmProviderSummary> for ProviderSummary {
    fn from(summary: LlmProviderSummary) -> Self {
        Self {
            id: summary.id.to_string(),
            kind: summary.kind.into(),
            display_name: summary.display_name,
            base_url: summary.base_url,
            models: summary.models,
            default_model: summary.default_model,
            is_default: summary.is_default,
            extra_headers: summary.extra_headers,
            secret_status: summary.secret_status,
        }
    }
}
```

- [ ] **Step 5: Update ProviderRecord struct**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRecord {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,           // Changed
    pub default_model: String,         // New
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}
```

- [ ] **Step 6: Update LlmProviderRecord → ProviderRecord conversion**

```rust
impl From<LlmProviderRecord> for ProviderRecord {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id.to_string(),
            kind: record.kind.into(),
            display_name: record.display_name,
            base_url: record.base_url,
            api_key: record.api_key.expose_secret().to_string(),
            models: record.models,
            default_model: record.default_model,
            is_default: record.is_default,
            extra_headers: record.extra_headers,
            secret_status: record.secret_status,
        }
    }
}
```

- [ ] **Step 7: Update build_provider_reentry_record helper**

```rust
fn build_provider_reentry_record(summary: LlmProviderSummary) -> ProviderRecord {
    ProviderRecord {
        id: summary.id.to_string(),
        kind: summary.kind.into(),
        display_name: summary.display_name,
        base_url: summary.base_url,
        api_key: String::new(),
        models: summary.models,
        default_model: summary.default_model,
        is_default: summary.is_default,
        extra_headers: summary.extra_headers,
        secret_status: summary.secret_status,
    }
}
```

- [ ] **Step 8: Update test_provider_connection command**

```rust
#[tauri::command]
pub async fn test_provider_connection(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
    model: String,  // New parameter
) -> Result<ProviderTestResult, String> {
    ctx.test_provider_connection(&LlmProviderId::new(id), &model)
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 9: Update test_provider_input command**

```rust
#[tauri::command]
pub async fn test_provider_input(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: ProviderInput,
    model: String,  // New parameter
) -> Result<ProviderTestResult, String> {
    ctx.test_provider_record(record.into(), &model)
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 10: Add model_override to create_chat_session**

```rust
#[tauri::command]
pub async fn create_chat_session(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    template_id: String,
    provider_preference_id: Option<String>,
    model_override: Option<String>,  // New parameter
) -> Result<ChatSessionPayload, String> {
    // ... existing code ...

    // After getting provider record, determine effective model
    let effective_model = model_override
        .as_ref()
        .filter(|m| !m.is_empty())
        .cloned()
        .unwrap_or_else(|| provider_record.default_model.clone());

    // Validate model is in provider's list
    if !provider_record.models.contains(&effective_model) {
        return Err(format!(
            "Model '{}' is not available in provider '{}'",
            effective_model, provider_record.id
        ));
    }

    // Use get_provider_with_model instead of get_provider
    let provider = ctx.get_provider_with_model(&provider_id, &effective_model)
        .await
        .map_err(|e| e.to_string())?;

    // ... rest of implementation
}
```

- [ ] **Step 11: Update tests in commands.rs**

Update all test ProviderInput/Record constructions:

```rust
// Old:
model: "gpt-4.1".to_string(),

// New:
models: vec!["gpt-4.1".to_string()],
default_model: "gpt-4.1".to_string(),
```

- [ ] **Step 12: Run tests**

Run: `cargo test -p desktop`
Expected: All tests pass

- [ ] **Step 13: Commit Tauri commands**

```bash
git add crates/desktop/src-tauri/src/commands.rs
git commit -m "$(cat <<'EOF'
feat(desktop): update Tauri commands for multi-model providers

- Update ProviderInput/Summary/Record types with models/default_model
- Add model parameter to test_provider_connection/input
- Add model_override to create_chat_session

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 6: Frontend Types and API

### Task 8: Update Frontend Types

**Files:**
- Modify: `crates/desktop/lib/tauri.ts`

- [ ] **Step 1: Update LlmProviderSummary interface**

```typescript
export interface LlmProviderSummary {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  models: string[];           // Changed from model: string
  default_model: string;      // New
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}
```

- [ ] **Step 2: Update LlmProviderRecord interface**

```typescript
export interface LlmProviderRecord {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];           // Changed from model: string
  default_model: string;      // New
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}
```

- [ ] **Step 3: Update ProviderInput interface**

```typescript
export interface ProviderInput {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];           // Changed from model: string
  default_model: string;      // New
  is_default: boolean;
  extra_headers: Record<string, string>;
}
```

- [ ] **Step 4: Update providers.testConnection function**

```typescript
  testConnection: (id: string, model: string) =>
    invoke<ProviderTestResult>("test_provider_connection", { id, model }),
```

- [ ] **Step 5: Update providers.testInput function**

```typescript
  testInput: (record: ProviderInput, model: string) =>
    invoke<ProviderTestResult>("test_provider_input", { record, model }),
```

- [ ] **Step 6: Update chat.createChatSession function**

```typescript
  createChatSession: (
    templateId: string,
    providerPreferenceId: string | null,
    modelOverride: string | null,
  ) =>
    invoke<ChatSessionPayload>("create_chat_session", {
      templateId,
      providerPreferenceId,
      modelOverride,
    }),
```

- [ ] **Step 7: Add effective_model to ChatSessionPayload**

```typescript
export interface ChatSessionPayload {
  session_key: string;
  template_id: string;
  runtime_agent_id: string;
  thread_id: string;
  effective_provider_id: string;
  effective_model: string;  // New - the model being used
}
```

- [ ] **Step 8: Commit frontend types**

```bash
git add crates/desktop/lib/tauri.ts
git commit -m "$(cat <<'EOF'
feat(desktop): update TypeScript types for multi-model providers

- Replace model with models/default_model in all interfaces
- Add model parameter to testConnection/testInput
- Add modelOverride to createChatSession
- Add effective_model to ChatSessionPayload

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 7: Frontend Components

### Task 9: Update Provider Form Dialog

**Files:**
- Modify: `crates/desktop/components/settings/provider-form-dialog.tsx`

- [ ] **Step 1: Update LlmProviderRecord interface in file**

```typescript
export interface LlmProviderRecord {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];
  default_model: string;
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}
```

- [ ] **Step 2: Add model input state**

Add after formData state:

```typescript
  const [newModel, setNewModel] = React.useState("");
```

- [ ] **Step 3: Add model management functions**

```typescript
  const handleAddModel = React.useCallback(() => {
    const trimmed = newModel.trim();
    if (!trimmed) return;
    if (formData.models.includes(trimmed)) {
      setNewModel("");
      return;
    }
    const newModels = [...formData.models, trimmed];
    setFormData({
      ...formData,
      models: newModels,
      default_model: formData.default_model || trimmed,
    });
    setNewModel("");
  }, [formData, newModel]);

  const handleRemoveModel = React.useCallback((model: string) => {
    const newModels = formData.models.filter((m) => m !== model);
    setFormData({
      ...formData,
      models: newModels,
      default_model: newModels.includes(formData.default_model)
        ? formData.default_model
        : newModels[0] || "",
    });
  }, [formData]);

  const handleSetDefaultModel = React.useCallback((model: string) => {
    setFormData({ ...formData, default_model: model });
  }, [formData]);
```

- [ ] **Step 4: Update initial formData state**

```typescript
  const [formData, setFormData] = React.useState<LlmProviderRecord>(
    () =>
      provider || {
        id: "",
        kind: "openai-compatible",
        display_name: "",
        base_url: "",
        api_key: "",
        models: [],
        default_model: "",
        is_default: false,
        extra_headers: {},
        secret_status: "ready",
      },
  );
```

- [ ] **Step 5: Update useEffect for provider changes**

```typescript
  React.useEffect(() => {
    if (provider) {
      setFormData(provider);
    } else {
      setFormData({
        id: "",
        kind: "openai-compatible",
        display_name: "",
        base_url: "",
        api_key: "",
        models: [],
        default_model: "",
        is_default: false,
        extra_headers: {},
        secret_status: "ready",
      });
    }
    // ... rest of effect
  }, [provider]);
```

- [ ] **Step 6: Update canTest condition**

```typescript
  const canTest = Boolean(
    formData.base_url.trim() &&
    formData.api_key.trim() &&
    formData.models.length > 0,
  );
```

- [ ] **Step 7: Update handleTestConnection**

```typescript
  const handleTestConnection = async () => {
    const record: ProviderInput = {
      id: formData.id,
      kind: formData.kind,
      display_name: formData.display_name,
      base_url: formData.base_url,
      api_key: formData.api_key,
      models: formData.models,
      default_model: formData.default_model,
      is_default: formData.is_default,
      extra_headers: formData.extra_headers,
    };
    const modelToTest = formData.default_model || formData.models[0];
    setTestDialogOpen(true);
    setTestingConnection(true);
    setTestResult(null);
    try {
      const result = await providers.testInput(record, modelToTest);
      setTestResult(result);
    } catch (error) {
      setTestResult({
        provider_id: record.id,
        model: modelToTest,
        base_url: record.base_url,
        checked_at: new Date().toISOString(),
        latency_ms: 0,
        status: "request_failed",
        message: error instanceof Error ? error.message : String(error),
      });
      console.error("Failed to test provider draft:", error);
    } finally {
      setTestingConnection(false);
    }
  };
```

- [ ] **Step 8: Replace model input with tag list UI**

Replace the model input div with:

```tsx
          <div className="space-y-2">
            <Label>Models</Label>
            <div className="flex flex-wrap gap-2 mb-2">
              {formData.models.map((model) => (
                <Badge
                  key={model}
                  variant={model === formData.default_model ? "default" : "secondary"}
                  className="cursor-pointer pr-1"
                  onClick={() => handleSetDefaultModel(model)}
                >
                  {model}
                  {model === formData.default_model && (
                    <span className="ml-1 text-[10px] opacity-70">默认</span>
                  )}
                  <button
                    type="button"
                    className="ml-1 hover:text-destructive"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleRemoveModel(model);
                    }}
                  >
                    ×
                  </button>
                </Badge>
              ))}
            </div>
            <div className="flex gap-2">
              <Input
                value={newModel}
                onChange={(e) => setNewModel(e.target.value)}
                placeholder="输入模型名称"
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    handleAddModel();
                  }
                }}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddModel}
                disabled={!newModel.trim()}
              >
                添加
              </Button>
            </div>
            <p className="text-[11px] text-muted-foreground">
              点击标签设为默认模型
            </p>
          </div>
```

- [ ] **Step 9: Add Badge import**

```typescript
import { Badge } from "@/components/ui/badge";
```

- [ ] **Step 10: Update draftProvider for test dialog**

```typescript
  const draftProvider = {
    id: formData.id,
    kind: formData.kind,
    display_name: formData.display_name || "未命名 Provider",
    base_url: formData.base_url,
    models: formData.models,
    default_model: formData.default_model,
    is_default: formData.is_default,
    extra_headers: formData.extra_headers,
    secret_status: formData.secret_status,
  };
```

- [ ] **Step 11: Run frontend build**

Run: `cd crates/desktop && pnpm build`
Expected: No errors

- [ ] **Step 12: Commit provider form changes**

```bash
git add crates/desktop/components/settings/provider-form-dialog.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add model tag list to provider form

Replace single model input with:
- Tag list showing all configured models
- Click tag to set as default
- Add/remove model functionality
- Validate at least one model before save/test

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

### Task 10: Update Provider Test Dialog

**Files:**
- Modify: `crates/desktop/components/settings/provider-test-dialog.tsx`

- [ ] **Step 1: Add model selector state**

```typescript
interface ProviderTestDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  provider: LlmProviderSummary | null;
  result?: ProviderTestResult | null;
  testing?: boolean;
  selectedModel?: string;
  onModelChange: (model: string) => void;
  onRetest: () => void;
}
```

- [ ] **Step 2: Add Select imports**

```typescript
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
```

- [ ] **Step 3: Add model selector to dialog**

Add after the provider info section and before the test button:

```tsx
        {provider && provider.models.length > 0 && (
          <div className="space-y-2">
            <Label className="text-xs text-muted-foreground">选择测试模型</Label>
            <Select
              value={selectedModel || provider.default_model}
              onValueChange={onModelChange}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="选择模型" />
              </SelectTrigger>
              <SelectContent>
                {provider.models.map((model) => (
                  <SelectItem key={model} value={model}>
                    {model}
                    {model === provider.default_model && " (默认)"}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
```

- [ ] **Step 4: Update model display in info section**

```tsx
              <div className="flex items-start justify-between gap-3">
                <span className="text-muted-foreground">Model</span>
                <span className="font-mono text-right">
                  {selectedModel ?? provider?.default_model ?? result?.model ?? "-"}
                </span>
              </div>
```

- [ ] **Step 5: Commit test dialog changes**

```bash
git add crates/desktop/components/settings/provider-test-dialog.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): add model selector to provider test dialog

Allow users to select which model to test when provider has multiple models.
Show (默认) label next to default model in dropdown.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

### Task 11: Update Providers Page

**Files:**
- Modify: `crates/desktop/app/settings/providers/page.tsx`

- [ ] **Step 1: Update LlmProviderRecord interface**

Remove local interface and import from component:

```typescript
import {
  ProviderCard,
  ProviderFormDialog,
  ProviderTestDialog,
  type LlmProviderRecord,
  DeleteConfirmDialog,
  Breadcrumb,
} from "@/components/settings";
```

- [ ] **Step 2: Add selected model state for test dialog**

```typescript
  const [testSelectedModel, setTestSelectedModel] = React.useState<string | null>(null);
```

- [ ] **Step 3: Update handleTestConnection**

```typescript
  const handleTestConnection = React.useCallback(
    (id: string) => {
      const provider = providerList.find((item) => item.id === id);
      if (provider?.secret_status === "requires_reentry") {
        return;
      }
      setActiveProviderId(id);
      setTestSelectedModel(provider?.default_model || null);
      setTestDialogOpen(true);
      void runConnectionTest(id, provider?.default_model || "");
    },
    [providerList, runConnectionTest],
  );
```

- [ ] **Step 4: Update runConnectionTest to accept model**

```typescript
  const runConnectionTest = React.useCallback(
    async (id: string, model: string) => {
      const provider = providerList.find((item) => item.id === id);
      setTestingProviderId(id);
      try {
        const result = await providers.testConnection(id, model);
        setTestResultsByProviderId((current) => ({ ...current, [id]: result }));
      } catch (error) {
        const fallbackResult: ProviderTestResult = {
          provider_id: id,
          model,
          base_url: provider?.base_url ?? "",
          checked_at: new Date().toISOString(),
          latency_ms: 0,
          status: "request_failed",
          message: error instanceof Error ? error.message : String(error),
        };
        setTestResultsByProviderId((current) => ({
          ...current,
          [id]: fallbackResult,
        }));
        console.error("Failed to test provider connection:", error);
      } finally {
        setTestingProviderId((current) => (current === id ? null : current));
      }
    },
    [providerList],
  );
```

- [ ] **Step 5: Update handleRetest**

```typescript
  const handleRetest = React.useCallback(() => {
    if (!activeProviderId || !testSelectedModel) return;
    void runConnectionTest(activeProviderId, testSelectedModel);
  }, [activeProviderId, testSelectedModel, runConnectionTest]);
```

- [ ] **Step 6: Update handleViewStatus**

```typescript
  const handleViewStatus = React.useCallback((id: string) => {
    const provider = providerList.find((item) => item.id === id);
    setActiveProviderId(id);
    setTestSelectedModel(provider?.default_model || null);
    setTestDialogOpen(true);
  }, [providerList]);
```

- [ ] **Step 7: Update handleSubmit for new fields**

```typescript
  const handleSubmit = async (record: LlmProviderRecord) => {
    const input: ProviderInput = {
      id: record.id,
      kind: record.kind,
      display_name: record.display_name,
      base_url: record.base_url,
      api_key: record.api_key,
      models: record.models,
      default_model: record.default_model,
      is_default: record.is_default,
      extra_headers: record.extra_headers,
    };
    await providers.upsert(input);
    setEditingProvider(null);
    await loadProviders();
  };
```

- [ ] **Step 8: Update handleEdit**

```typescript
  const handleEdit = async (id: string) => {
    const provider = await providers.get(id);
    if (provider) {
      const formRecord: LlmProviderRecord = {
        id: provider.id,
        kind: provider.kind,
        display_name: provider.display_name,
        base_url: provider.base_url,
        api_key:
          typeof provider.api_key === "string"
            ? provider.api_key
            : (provider.api_key as { api_key: string }).api_key || "",
        models: provider.models,
        default_model: provider.default_model,
        is_default: provider.is_default,
        extra_headers: provider.extra_headers,
        secret_status: provider.secret_status,
      };
      setEditingProvider(formRecord);
    }
  };
```

- [ ] **Step 9: Update ProviderTestDialog props**

```tsx
      <ProviderTestDialog
        open={testDialogOpen}
        onOpenChange={(open) => {
          setTestDialogOpen(open);
          if (!open) {
            setActiveProviderId(null);
            setTestSelectedModel(null);
          }
        }}
        provider={activeProvider}
        result={activeTestResult}
        selectedModel={testSelectedModel || undefined}
        onModelChange={(model) => setTestSelectedModel(model)}
        testing={
          activeProviderId !== null && testingProviderId === activeProviderId
        }
        onRetest={handleRetest}
      />
```

- [ ] **Step 10: Commit providers page changes**

```bash
git add crates/desktop/app/settings/providers/page.tsx
git commit -m "$(cat <<'EOF'
feat(desktop): update providers page for multi-model support

- Add model selection state for test dialog
- Update all handlers to pass selected model to API
- Update form submission with models/default_model fields

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Chunk 8: Final Integration and Testing

### Task 12: Update Remaining Tests

**Files:**
- Modify: `crates/claw/tests/db_sqlite_llm_repository.rs`
- Modify: `crates/claw/tests/llm_manager.rs`

- [ ] **Step 1: Update build_record helper in db_sqlite_llm_repository.rs**

In `crates/claw/tests/db_sqlite_llm_repository.rs`, update the `build_record` function (lines 12-24):

```rust
fn build_record(id: &str, display_name: &str, is_default: bool) -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(id),
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: display_name.to_string(),
        base_url: format!("https://{id}.example.com/v1"),
        api_key: SecretString::new(format!("sk-{id}")),
        models: vec!["gpt-4o-mini".to_string()],  // Changed from model
        default_model: "gpt-4o-mini".to_string(),  // New field
        is_default,
        extra_headers: HashMap::new(),
        secret_status: ProviderSecretStatus::Ready,
    }
}
```

- [ ] **Step 2: Update build_record_with_headers helper**

Update the `build_record_with_headers` function (lines 26-43):

```rust
fn build_record_with_headers(
    id: &str,
    display_name: &str,
    is_default: bool,
    headers: HashMap<String, String>,
) -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(id),
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: display_name.to_string(),
        base_url: format!("https://{id}.example.com/v1"),
        api_key: SecretString::new(format!("sk-{id}")),
        models: vec!["gpt-4o-mini".to_string()],  // Changed from model
        default_model: "gpt-4o-mini".to_string(),  // New field
        is_default,
        extra_headers: headers,
        secret_status: ProviderSecretStatus::Ready,
    }
}
```

- [ ] **Step 3: Run tests to verify**

Run: `cargo test -p claw --features dev db_sqlite_llm`
Expected: All tests pass

- [ ] **Step 4: Commit test updates**

```bash
git add crates/claw/tests/
git commit -m "$(cat <<'EOF'
test(claw): update tests for multi-model provider schema

Update all test LlmProviderRecord constructions to use
models/default_model fields instead of single model field.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

### Task 13: Run Full Verification

- [ ] **Step 1: Run prek**

Run: `prek`
Expected: All checks pass

- [ ] **Step 2: Run cargo deny**

Run: `cargo deny check`
Expected: No issues

- [ ] **Step 3: Manual integration test**

1. Start the desktop app: `pnpm tauri dev`
2. Navigate to Settings → LLM Providers
3. Create a new provider with multiple models
4. Verify models display as tags
5. Click a tag to set as default
6. Test connection with different models
7. Verify model selection persists after save

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat: complete multi-model provider support

Allow single LLM provider to support multiple configurable models:
- Database schema with models array and default_model
- Backend validation and model-aware API methods
- Frontend tag list UI and model selector in test dialog

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
EOF
)"
```

---

## Summary

This plan implements multi-model provider support through:

1. **Database**: Migration adds `models` (JSON array) and `default_model` fields
2. **Backend**: Types updated through all layers with validation
3. **API**: Model parameter added to test and session creation endpoints
4. **Frontend**: Tag-based model management and test dialog model selector

Each chunk produces a working, testable state. Follow TDD principles where specified.
