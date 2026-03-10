# CLI Guide

- `src/main.rs` should stay thin: tracing setup, `Agent::init(...)`, then command dispatch.
- Dev-only tooling lives behind the `dev` feature in `src/dev.rs`.
- `llm complete` defaults to the configured default provider; `--provider <id>` is only for override.
- Stream mode must print reasoning and answer separately:
  - `[Reasoning] ...`
  - `[Summary] ...`
- `provider import` reads TOML from `src/dev/config.rs`; keep import/export formats stable unless storage shape changes.
- Prefer surfacing agent/LLM errors directly instead of inventing CLI-only fallback behavior.
