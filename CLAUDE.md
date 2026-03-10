# ArgusClaw Development Guide
## Build & Test

```bash
cargo fmt                                                    # format
cargo clippy --all --benches --tests --examples --all-features  # lint (zero warnings)
cargo test                                                   # unit tests
cargo test --features integration                            # + Sqlite tests
RUST_LOG=argusclaw=debug,agent=debug cargo run  # run with logging
```

## Code Style

- Prefer `crate::` for cross-module imports; `super::` is fine in tests and intra-module refs
- No `pub use` re-exports unless exposing to downstream consumers
- No `.unwrap()` or `.expect()` in production code (tests are fine)
- Use `thiserror` for error types in `error.rs`
- Map errors with context: `.map_err(|e| SomeError::Variant { reason: e.to_string() })?`
- Prefer strong types over strings (enums, newtypes)
- Keep functions focused, extract helpers when logic is reused
- Comments for non-obvious logic only

## Architecture
Prefer generic/extensible architectures over hardcoding specific integrations. Ask clarifying questions about the desired abstraction level before implementing.

Key traits for extensibility: Database, Channel, Tool, LlmProvider。

All I/O is async with tokio. Use Arc<T> for shared state, RwLock for concurrent access.

## Project Structure

```text
crates/
├── agent/
│   ├── src/
│   │   ├── lib.rs                    # Library root, module declarations and exports
│   │   ├── error.rs                  # Top-level agent error types
│   │   ├── agent.rs                  # Agent root object; owns LLMManager
│   │   ├── db/                       # Storage abstractions and implementations
│   │   │   ├── mod.rs                # DB module entry point and shared DB errors
│   │   │   ├── llm.rs                # LLM provider records and repository trait
│   │   │   └── sqlite/               # SQLx-backed SQLite implementation
│   │   │       ├── mod.rs            # SQLite connect/migrate helpers
│   │   │       └── llm.rs            # SQLite LLM provider repository
│   │   └── llm/                      # LLM domain types, manager, and provider implementations
│   │       ├── mod.rs                # LLM module entry point and re-exports
│   │       ├── error.rs              # Provider-agnostic LLM errors
│   │       ├── manager.rs            # LLMManager: list providers and build provider instances
│   │       ├── provider.rs           # Core LlmProvider trait and request/response types
│   │       ├── retry.rs              # Retry wrapper for LlmProvider
│   │       ├── secret.rs             # Host-bound API key encryption/decryption
│   │       └── providers/            # Concrete provider implementations
│   │           ├── mod.rs            # Provider module exports
│   │           └── openai_compatible.rs # OpenAI-compatible provider factory and implementation
│   ├── migrations/                   # SQLx migrations
│   └── tests/                        # E2E tests only; multi-module scenarios that do not fit inline tests
└── cli/
    ├── CLAUDE.md                      # CLI module guide
    └── src/
        ├── main.rs                    # CLI bootstrap: tracing, DB init, Agent startup
        ├── dev.rs                     # Dev-only commands (behind `dev` feature)
        └── dev/
            └── config.rs              # Provider import TOML format
```

## Testing

- Prefer colocated tests with `#[cfg(test)]` in the same file as the implementation
- Use `crates/*/tests/` only for E2E-style coverage that exercises multiple modules together

## DB

- Default `DATABASE_URL` is `~/.argusclaw/sqlite.db`
