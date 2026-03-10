# ArgusClaw Development Guide
## Build & Test

```bash
cargo fmt                                                    # format
cargo clippy --all --benches --tests --examples --all-features  # lint (zero warnings)
cargo test                                                   # unit tests
cargo test --features integration                            # + Sqlite tests
RUST_LOG=ironclaw=debug cargo run                            # run with logging
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

## LLM & DB Conventions

- Put storage entry points under `crates/agent/src/db/`
- Keep LLM storage traits and records in `crates/agent/src/db/llm.rs`
- Keep LLM provider implementations under `crates/agent/src/llm/`
- Keep SQLite implementations under `crates/agent/src/db/sqlite/`
- Use `sqlx` for SQLite access and schema management; migrations live in `crates/agent/migrations/`
- `LLMManager` is responsible for listing providers and constructing concrete `LlmProvider` instances from stored records
- OpenAI-compatible providers are modeled as one provider kind to many concrete provider records
- `Agent` owns `LLMManager`; CLI bootstrap is responsible for tracing initialization, database connection, and migration execution
- Default `DATABASE_URL` is `~/.argusclaw/sqlite.db`
- Never store provider API keys in plaintext; encrypt/decrypt them using host MAC-derived key material with mature cross-platform libraries
