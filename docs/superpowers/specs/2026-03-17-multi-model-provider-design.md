# Multi-Model LLM Provider Support

## Summary

Enable a single LLM Provider to support multiple models, allowing users to configure and select different models within the same provider configuration. This reduces the need to create duplicate providers for different models from the same API endpoint.

## Motivation

Currently, each `llm_providers` record supports only one model. Users who want to use multiple models from the same provider (e.g., OpenAI's `gpt-4.1`, `gpt-4.1-mini`, `o3`) must create separate provider records with identical configuration except for the model name. This leads to:

- Redundant configuration (same base_url, api_key repeated)
- More management overhead
- Inefficient credential storage (same API key encrypted multiple times)

## Design

### Database Schema

**Migration**: Add `models` and `default_model` fields, migrate existing `model` data, then remove the `model` field.

**Final schema**:

```sql
CREATE TABLE llm_providers (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    models TEXT NOT NULL DEFAULT '[]',      -- JSON array: ["gpt-4.1", "gpt-4.1-mini"]
    default_model TEXT NOT NULL,            -- Default model: "gpt-4.1"
    encrypted_api_key BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    extra_headers TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

**Migration strategy** (SQLite requires table rebuild for column removal):

1. Create new table with updated schema
2. Copy data: `models = json_array(model)`, `default_model = model`
3. Drop old table
4. Rename new table
5. Recreate indexes

### Backend Types

**`crates/claw/src/db/llm.rs`**:

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

**Validation**:
- `models` must be non-empty
- `default_model` must exist in `models`
- Enforced at repository layer during upsert

### API Changes

**`crates/desktop/src-tauri/src/commands.rs`**:

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

**New/Modified Commands**:

```rust
// Test connection with specific model
#[tauri::command]
pub async fn test_provider_connection(
    ctx: State<'_, Arc<AppContext>>,
    id: String,
    model: String,                     // New parameter
) -> Result<ProviderTestResult, String>

#[tauri::command]
pub async fn test_provider_input(
    ctx: State<'_, Arc<AppContext>>,
    record: ProviderInput,
    model: String,                     // New parameter
) -> Result<ProviderTestResult, String>

// Create session with model override
#[tauri::command]
pub async fn create_chat_session(
    ctx: State<'_, Arc<AppContext>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    template_id: String,
    provider_preference_id: Option<String>,
    model_override: Option<String>,    // New parameter
) -> Result<ChatSessionPayload, String>
```

### LLM Manager Changes

**`crates/claw/src/llm/manager.rs`**:

```rust
impl LLMManager {
    /// Get provider configured with a specific model
    pub async fn get_provider_with_model(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self.repository.get_provider(id).await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })?;

        // Validate model exists in provider's models list
        if !record.models.contains(&model.to_string()) {
            return Err(AgentError::ModelNotAvailable {
                provider: id.to_string(),
                model: model.to_string(),
            });
        }

        self.build_provider_with_model(record, model)
    }

    /// Get provider with default model
    pub async fn get_provider(&self, id: &LlmProviderId) -> Result<Arc<dyn LlmProvider>, AgentError> {
        let record = self.repository.get_provider(id).await?
            .ok_or_else(|| AgentError::ProviderNotFound { id: id.to_string() })?;

        self.build_provider_with_model(record, &record.default_model)
    }

    pub async fn test_provider_connection(
        &self,
        id: &LlmProviderId,
        model: &str,                     // New parameter
    ) -> Result<ProviderTestResult, AgentError> {
        // ... implementation with specified model
    }

    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,                     // New parameter
    ) -> Result<ProviderTestResult, AgentError> {
        // ... implementation with specified model
    }
}
```

### Frontend Changes

**`crates/desktop/components/settings/provider-form-dialog.tsx`**:

Replace single model input with:

1. **Model Tag List Component**:
   - Display models as removable tags (using shadcn Badge)
   - Input field + "Add" button to add new models
   - Click tag to remove
   - Validation: at least one model required

2. **Default Model Selector**:
   - Dropdown populated from `models` list
   - Automatically select first model as default when adding
   - Disable if no models configured

**Layout**:
```
┌─────────────────────────────────────┐
│ Models                              │
│ ┌─────┐ ┌─────────────┐ ┌────────┐ │
│ │ gpt-4.1 │ x │ │ gpt-4.1-mini │ x │ │ o3 │ x │
│ └─────┘ └─────────────┘ └────────┘ │
│ ┌────────────────────────┐ ┌─────┐ │
│ │ Enter model name...    │ │ Add │ │
│ └────────────────────────┘ └─────┘ │
├─────────────────────────────────────┤
│ Default Model                       │
│ ┌─────────────────────────────────┐ │
│ │ gpt-4.1                   ▼     │ │
│ └─────────────────────────────────┘ │
└─────────────────────────────────────┘
```

**`crates/desktop/components/settings/provider-test-dialog.tsx`**:

Add model selector above test button:

```
┌─────────────────────────────────────┐
│ Test Connection                     │
│                                     │
│ Select model to test:               │
│ ┌─────────────────────────────────┐ │
│ │ gpt-4.1                   ▼     │ │
│ └─────────────────────────────────┘ │
│                                     │
│ [Test Connection]                   │
│                                     │
│ Result: ✓ Connected (127ms)        │
└─────────────────────────────────────┘
```

**`crates/desktop/lib/tauri.ts`**:

Update TypeScript types:

```typescript
export interface LlmProviderSummary {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  models: string[];           // Changed
  default_model: string;      // New
  is_default: boolean;
  extra_headers: Record<string, string>;
  secret_status: ProviderSecretStatus;
}

export interface ProviderInput {
  id: string;
  kind: "openai-compatible";
  display_name: string;
  base_url: string;
  api_key: string;
  models: string[];           // Changed
  default_model: string;      // New
  is_default: boolean;
  extra_headers: Record<string, string>;
}

// Updated API functions
export async function testConnection(id: string, model: string): Promise<ProviderTestResult>
export async function testInput(record: ProviderInput, model: string): Promise<ProviderTestResult>
```

### Chat Session Model Selection

When creating a chat session, allow users to select which model to use:

1. **Session creation flow**:
   - User selects provider (or uses default)
   - Dropdown shows available models from that provider
   - If no selection, use `default_model`

2. **API parameter**:
   - `create_chat_session(template_id, provider_preference_id, model_override)`
   - `model_override` is optional; if not provided, use provider's `default_model`

### Error Handling

**New error cases**:

1. **Empty models list**:
   - Validation error during upsert: "At least one model is required"

2. **Invalid default_model**:
   - Validation error during upsert: "Default model must be in models list"

3. **Model not available**:
   - Runtime error when requesting unavailable model: `AgentError::ModelNotAvailable { provider, model }`

4. **Model test failure**:
   - `ProviderTestResult` includes the tested model name
   - Error message indicates which model failed

### Testing Strategy

1. **Unit tests**:
   - `LlmProviderRecord` serialization/deserialization with models array
   - Validation: empty models, invalid default_model
   - Migration: existing single model → models array + default_model

2. **Integration tests**:
   - Provider CRUD with multiple models
   - Test connection with specific model
   - Create session with model override

3. **E2E tests**:
   - Frontend: add/remove models, select default
   - Frontend: test connection with different models
   - Chat session: model selection

## Implementation Order

1. **Database migration** - Create and test migration script
2. **Backend types** - Update `LlmProviderRecord`, `LlmProviderSummary`
3. **Repository layer** - Update SQLite implementation
4. **LLM Manager** - Add model parameter support
5. **Tauri commands** - Update API endpoints
6. **Frontend types** - Update TypeScript interfaces
7. **Frontend components** - Model tag list, default selector, test dialog
8. **Tests** - Unit, integration, E2E

## Rollback Plan

If issues arise:

1. Migration creates backup of original table
2. Rollback migration restores original schema
3. Frontend falls back to single model input if API returns error
4. Backend validates and returns clear error messages

## Future Considerations

- **Model auto-discovery**: Fetch available models from provider API
- **Model-specific settings**: Different temperature/max_tokens per model
- **Model aliases**: User-friendly names for models
- **Model capabilities**: Store context window, features per model
