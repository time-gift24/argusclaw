# Token Fetch Integration Guide

## Overview

After login, the app automatically creates an LLM provider (`TokenLLMProvider`) that injects a bearer token into every request. Token is fetched from your auth server on login and refreshed every 5 minutes.

## Flow

```
User Login → fetch_token (username + password) → get JWT
                                         ↓
LLM Request → TokenLLMProvider → injects "Authorization: Bearer <token>"
                                         ↓
5 min later → TokenLLMProvider → fetch_token again → get fresh JWT
```

## Configuration

Edit `crates/desktop/src-tauri/src/commands.rs`:

```rust
// === Hardcoded config (TODO: make configurable later) ===
const AUTH_BASE_URL: &str = "http://localhost:8080";              // LLM API base URL
const TOKEN_URL: &str = "http://localhost:8080/api/auth/token";    // Token fetch endpoint
const TOKEN_HEADER_NAME: &str = "Authorization";                   // Header name for token
const TOKEN_HEADER_PREFIX: &str = "Bearer ";                      // Header value prefix
const AUTH_MODEL: &str = "gpt-4o-mini";                           // Default model
```

## Auth Server Requirements

Your auth server must implement the token endpoint:

### Request

```
POST /api/auth/token
Content-Type: application/json

{
  "username": "alice",
  "password": "secret"
}
```

### Success Response (HTTP 200)

```json
{
  "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
}
```

### Failure Response

Any non-2xx status returns a login error to the user.

## LLM Request Headers

Every LLM request from `TokenLLMProvider` will include:

```
POST <AUTH_BASE_URL>/v1/chat/completions
Authorization: Bearer <token from auth server>
Content-Type: application/json
```

## Debugging

Enable debug logging:

```bash
RUST_LOG=arguswing=debug,argus=debug cargo run -p desktop
```

Look for `fetch_token` logs on login and every 5 minutes.

## Future Work

- [ ] Move config to a settings file / environment variables
- [ ] Configurable token refresh interval
- [ ] Per-user provider storage instead of single global provider
