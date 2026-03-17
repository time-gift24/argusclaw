# Agent Provider Model Binding Design

**Date:** 2026-03-17

## Context

Agent templates currently store only `provider_id`. The desktop settings page lets users choose an LLM provider, but not a concrete model. The chat surface separately supports a temporary provider/model selection, and `create_chat_session` returns an `effective_model`, but the runtime agent is still created from the provider's default model.

That leaves three mismatches:
- Agent configuration cannot express the concrete model users intend to use.
- Chat defaults do not follow the agent template's model preference.
- Temporary model overrides affect session metadata, but not the runtime provider binding itself.

The product requirement is to make an agent template optionally bind to a concrete `provider + model` pair while still allowing users to override the model temporarily in chat.

## Decision

Add an optional `model` field to agent templates and treat `provider_id + model` as a pair.

Rules:
- If `provider_id` is empty, `model` must also be empty.
- If `provider_id` is set, `model` must be set to one of that provider's `models`.
- Built-in `arguswing` remains empty for both fields and resolves at runtime to the default provider and its default model.

## Chosen UX

### Agent Settings
- Agent editor changes from a single provider selector to a two-step binding:
  - optional provider selector
  - model selector scoped to the chosen provider
- Clearing the provider clears the model.
- Selecting a provider defaults the model to that provider's `default_model`.
- If the stored model is no longer available on the selected provider, the editor surfaces that mismatch and requires reselection before save.

### Chat Runtime
- Chat sessions should default to the template's saved provider/model binding.
- Users may still temporarily switch provider/model from the chat toolbar.
- Temporary chat selection overrides the template binding only for the current session variant.

## Runtime Resolution Order

When creating a runtime agent from a template:
1. If chat provides a temporary provider/model override, use that.
2. Otherwise, if the template has `provider_id + model`, use that.
3. Otherwise, use the application's default provider and its `default_model`.

Partial states are normalized:
- override provider without override model => provider's default model
- template provider without template model => invalid persisted state; reject and surface a clear error
- override model without provider => use template provider if present, else the app default provider

## Data Model Changes

### Agent Domain Model
Add optional `model` to `AgentRecord` and related DTOs.

### SQLite Schema
Add nullable `model TEXT` column to `agents`.

Existing rows are left as `NULL`.
- `arguswing` remains `NULL`
- older user-created agents remain `NULL` until edited

No data backfill is needed because the desired semantics for missing data are still valid runtime fallback.

## Backend Behavior

### Repository Layer
- persist `agents.model`
- round-trip `NULL` <-> `None`

### AppContext / AgentManager
- runtime creation must bind to a provider instance created with the resolved model, not merely the provider default
- `create_runtime_agent_from_template` should resolve both effective provider and effective model before constructing the runtime agent
- `RuntimeAgentHandle` should include `effective_model`

### Validation
On upsert of an agent template:
- if `provider_id` is empty and `model` is present -> reject
- if `provider_id` is set and `model` is absent -> reject
- if both are set, ensure the model exists in the provider summary/record

This validation should happen in the application layer so both desktop/Tauri and future clients share the same rules.

## Desktop / Tauri Changes

### Tauri DTOs
- `AgentInput` and `AgentRecord` payloads gain optional `model`
- `create_chat_session` can keep the current signature, but runtime creation must apply the selected model override instead of merely echoing it in the payload

### Desktop Agent Editor
- replace provider-only selector with:
  - provider select
  - model select populated from selected provider
- create-mode defaults:
  - if a preferred provider exists, preselect it and its default model
  - users can still clear both to keep runtime fallback behavior
- save is allowed when:
  - display name and system prompt are present
  - and provider/model are either both empty or both valid

### Desktop Cards
- Agent cards should display `Provider / Model` when configured
- if unbound, keep showing fallback wording such as `未指定`

### Chat Store And Selectors
- session keys should include model override as well as provider preference; otherwise different model selections on the same provider would incorrectly reuse the same session
- provider selector continues to allow temporary switching by selecting a concrete provider/model pair
- when no override exists, the displayed effective provider/model should come from the active session, which now reflects the template binding

## Testing Strategy

### Rust
- repository test: `agents.model` round-trips through SQLite
- Tauri command test: `AgentInput` serializes/deserializes `model`
- App/runtime tests: template-bound model becomes runtime effective model
- validation tests: reject provider without model and model without provider

### Desktop
- Agent editor tests: provider/model pair UI and save logic
- chat store tests: session keys include model override
- Tauri binding tests: chat payload still exposes effective provider/model

### Migration
- add a schema migration for `agents.model`
- existing generated-id migration test should remain green after staging the new migration

## Risks And Mitigations

### Risk: Runtime still binds provider default model
Mitigation:
- resolve effective model before runtime agent creation
- add regression test that inspects the returned `effective_model`

### Risk: Session cache reuses the wrong model
Mitigation:
- include model override in the session key
- add source-level regression test in `chat-store-session-model.test.mjs`

### Risk: Invalid persisted provider/model combinations
Mitigation:
- validate on save
- show explicit editor guidance when the chosen provider no longer offers the stored model

## Success Criteria
- Agent templates can optionally store a concrete provider/model pair.
- Chat sessions default to the template's saved model.
- Temporary model switching still works and creates a distinct session variant.
- Runtime agents actually bind to the selected model rather than the provider default.
- `arguswing` and other unbound agents still fall back to the app default provider/model.
