# Axum Server Design

Date: 2026-04-06

## Summary

Build a new `axum`-based backend product for end users while preserving the existing `desktop` product as the internal development workbench.

The two products should share the same core runtime, agent execution, repository traits, and provider/tool infrastructure, but differ in authentication, permissions, and exposed capabilities:

- `desktop`: full local capability, existing login flow, provider management, MCP management, agent editing, and debugging workflows remain available.
- `server`: OAuth2 login, multi-user isolation, shared server-managed providers/MCP/agents, and only chat/task execution capabilities exposed to end users.

## Goals

- Add a new `axum` backend service exposing chat-oriented APIs.
- Use PostgreSQL 13.22 for the server product via `argus-repository`.
- Preserve `desktop` as a separate product with its existing full-power workflow.
- Support multi-user server execution with strict per-user isolation for user-owned runtime data.
- Move LLM provider token-exchange credentials out of end-user login state and into server-managed configuration.
- Define a production-ready OAuth2 abstraction with a development-only fake OAuth2 implementation.

## Non-Goals

- Replacing the existing `desktop` login flow with OAuth2.
- Exposing provider, MCP, or agent-editing APIs to ordinary server users.
- Rewriting the session/thread runtime from scratch.
- Building the production OAuth2 provider in this phase.

## Product Boundary

### Desktop

`desktop` remains an internal product for developing and validating agent behavior locally. It keeps:

- existing login flow
- provider CRUD and testing
- MCP CRUD and testing
- agent template editing and composition
- full local session/thread/tool debugging

Desktop should continue to use the shared core, but it should not be forced into the server's OAuth2 or multi-user model.

### Server

`server` is a separate product for end users. It exposes only:

- login/logout and current-user lookup
- listing enabled agents
- creating sessions
- creating and continuing chat threads
- streaming thread events
- canceling user-owned work
- reading user-owned history

The server should not expose provider, MCP, or agent authoring capabilities to ordinary users.

## Architecture

### Shared Core

Shared code should contain:

- session/thread/turn runtime
- job execution and thread pool
- `ThreadEvent` event model
- provider construction and testing
- MCP runtime/tool integration
- repository traits
- agent template loading

The shared layer should be organized around capability services instead of a single desktop-shaped facade:

- `UserChatServices`: user-facing session/thread/chat/task operations
- `AdminServices`: provider, MCP, and agent management operations

Product composition:

- `desktop` uses `UserChatServices` and `AdminServices`
- `server` uses `UserChatServices`
- future internal server admin pages may also use `AdminServices`

### Server Entry

Add a new `axum` crate as the server entrypoint. It will be responsible for:

- HTTP routing
- auth middleware
- cookie session management
- SSE event streaming
- dev OAuth2 pages and callback handling
- wiring shared services against PostgreSQL implementations

## Authentication Design

### Server Auth Model

Server login is OAuth2-based and maps to a minimal user record:

- `id`
- `external_subject`
- `account`
- `display_name`
- `created_at`

These fields are sufficient for the server product.

### OAuth2 Abstraction

Define a trait for server-side OAuth2 providers:

```rust
#[async_trait]
pub trait OAuth2AuthProvider: Send + Sync {
    async fn authorize_url(&self, state: String, redirect_uri: String) -> Result<String>;
    async fn exchange_code(
        &self,
        code: String,
        redirect_uri: String,
    ) -> Result<OAuth2Identity>;
}

pub struct OAuth2Identity {
    pub external_subject: String,
    pub account: String,
    pub display_name: String,
}
```

The production provider will be implemented later. For now, the abstraction must be stable enough that the dev provider can be replaced without changing route semantics.

### Dev OAuth2 Flow

Development mode uses a fake OAuth2 experience that matches real redirect/callback behavior:

1. `GET /auth/login`
   redirects to a local dev authorize page.
2. `GET /dev-oauth/authorize`
   renders a simple form for choosing or entering a test account.
3. submitting the form generates a short-lived authorization code.
4. browser redirects to `GET /auth/callback?code=...&state=...`.
5. server exchanges the code through `DevOAuth2Provider`.
6. server upserts the user by `external_subject`.
7. server establishes a cookie-backed session and redirects to the app.

This keeps the route contract close to production OAuth2 while avoiding external dependencies.

### Desktop Auth

Desktop keeps its current auth flow. Server-side OAuth2 traits must not leak into the desktop login path.

## LLM Provider Credential Model

### Problem

The current implementation couples two unrelated concepts:

- the local account used for desktop login
- the username/password used by some LLM providers to exchange for a token

That coupling works in a single-user desktop setup, but it is incorrect for the server:

- server end-user login is OAuth2 and does not include a password
- provider token-exchange credentials are server-managed secrets
- ordinary users should not own or know provider token credentials

### Target Model

Keep `TokenLLMProvider`, but change its credential source abstraction.

Add a new repository abstraction dedicated to provider token-exchange credentials, for example:

```rust
#[async_trait]
pub trait ProviderTokenCredentialRepository: Send + Sync {
    async fn get_credentials_for_provider(
        &self,
        provider_id: &LlmProviderId,
    ) -> Result<Option<ProviderTokenCredential>>;
}

pub struct ProviderTokenCredential {
    pub provider_id: LlmProviderId,
    pub username: String,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}
```

Then replace the current dependency chain:

- current:
  `TokenLLMProvider <- AccountTokenSource <- AccountRepository`
- target:
  `TokenLLMProvider <- StoredCredentialTokenSource <- ProviderTokenCredentialRepository`

### Migration Strategy

- keep `TokenLLMProvider` behavior: token caching, refresh, and header injection
- replace only its credential source
- keep `AccountRepository` and `AccountManager` for desktop local login
- stop using `AccountRepository` as the server's provider-credential source
- allow desktop to temporarily bridge old local storage if needed

### Provider Scope

Provider configuration is server-level and shared:

- all server users use the same configured providers
- provider token-exchange credentials are entered on the management side
- ordinary users only consume enabled agents backed by those providers

## Data Model

### Server-Level Shared Resources

- `users`
- `llm_providers`
- `provider_token_credentials`
- `mcp_servers`
- `agent_templates`
- `agent_mcp_bindings`

`agent_templates` gains:

- `is_enabled: bool`

Ordinary users only see templates where `is_enabled = true`.

### User-Owned Resources

These are isolated by `user_id` on the server:

- `sessions`
- `threads`
- `turn logs`
- `jobs`
- runtime artifacts and persisted metadata

Isolation rule:

- a user can create, list, rename, cancel, and inspect only resources they own

## API Design

### Auth API

- `GET /auth/login`
- `GET /auth/callback`
- `POST /auth/logout`
- `GET /api/me`

`GET /api/me` returns:

- `id`
- `account`
- `display_name`

### User Chat API

- `GET /api/agents`
- `POST /api/sessions`
- `GET /api/sessions`
- `PATCH /api/sessions/:session_id`
- `GET /api/sessions/:session_id/threads`
- `GET /api/threads/:thread_id`
- `PATCH /api/threads/:thread_id`
- `POST /api/threads/:thread_id/messages`
- `POST /api/threads/:thread_id/cancel`
- `GET /api/threads/:thread_id/events`

API principles:

- ordinary users do not receive provider/MCP/agent authoring endpoints
- route semantics should stay close to current desktop chat flows
- event streaming should reuse the existing `ThreadEvent` model through SSE

## Repository Design

### New Traits

- `UserRepository`
- `ProviderTokenCredentialRepository`

Suggested responsibilities:

- `UserRepository`
  - get by external subject
  - upsert OAuth2 user
  - get by id
- `ProviderTokenCredentialRepository`
  - read and write server-managed token-exchange credentials

### Extended Traits

Extend existing runtime repositories with ownership-aware methods for server use:

- `SessionRepository`
  - create for user
  - list for user
  - rename/get/delete with owner checks
- `ThreadRepository`
  - list/get for user
  - append and mutate through owner-checked paths
- `JobRepository`
  - inspect and cancel only through owner-aware lookups

Desktop can continue to use local implementations that do not need remote multi-user semantics.

## PostgreSQL Design

Add PostgreSQL implementations under `argus-repository`:

- `postgres/mod.rs`
- `postgres/user.rs`
- `postgres/session.rs`
- `postgres/thread.rs`
- `postgres/job.rs`
- `postgres/llm_provider.rs`
- `postgres/provider_token_credential.rs`
- `postgres/mcp.rs`
- `postgres/agent.rs`

Rules:

- SQL lives only in `argus-repository`
- upper layers depend only on traits
- desktop may keep SQLite
- server uses PostgreSQL 13.22

## Runtime Behavior

The session/thread runtime should be preserved, not rewritten.

The important migration is:

- from desktop-local invoke calls to HTTP/SSE on server
- from implicit single user to explicit authenticated ownership

This preserves the current experience model:

- choose agent
- create session
- send message
- stream turn events
- inspect history
- cancel work

## Error Handling

- unauthenticated requests return auth errors before reaching chat services
- ownership violations return not-found or forbidden responses without leaking other users' resource existence
- provider token fetch failures surface as provider execution errors
- disabled agents should not be listable or invokable by ordinary users
- dev OAuth2 code reuse/expiry should return clean callback failures

## Testing Strategy

### Shared / Service Tests

- listing enabled agents only
- user ownership checks for sessions/threads/jobs
- thread event delivery through service boundaries
- provider token credential resolution

### Repository Tests

- PostgreSQL integration tests for user ownership filtering
- provider token credential persistence
- user upsert by `external_subject`

### Auth Tests

- dev OAuth2 authorize/callback happy path
- cookie session establishment
- repeated login idempotence

### API Tests

- `GET /api/me`
- session/thread lifecycle
- message send + SSE event streaming
- canceling user-owned work
- unauthenticated and cross-user access rejection

### Desktop Regression Tests

- current local login path still works
- provider token flows still work in desktop mode
- admin/debug capabilities remain available

## Delivery Plan

### Phase 1: Shared Core Refactor

- extract user chat capabilities from desktop-shaped entrypoints
- introduce user-aware service boundaries
- add OAuth2 abstraction
- add provider token credential abstraction

### Phase 2: PostgreSQL Support

- implement PostgreSQL repositories
- add migrations for users, ownership, and provider token credentials
- add `agent_templates.is_enabled`

### Phase 3: Axum Server

- create server crate
- implement auth routes
- implement user chat routes
- wire cookie sessions and SSE

### Phase 4: Hardening

- ownership and auth verification
- disabled-agent enforcement
- concurrency and stream validation
- desktop regression verification

## Recommendation

Proceed with an incremental implementation that preserves desktop behavior while introducing the new server product beside it. The critical design choice is to keep shared runtime logic but separate product semantics:

- desktop remains a full-power local workbench
- server becomes a constrained multi-user chat product
- provider token credentials become server-managed secrets rather than user-login data
