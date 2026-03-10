# LLM Module Guide

## Scope

This directory contains ArgusClaw's LLM-facing core types, provider management, provider implementations, and local provenance records for vendored upstream code.

## Structure

- `mod.rs`: LLM module entry point and re-exports
- `provider.rs`: provider-agnostic request/response types and the `LlmProvider` trait
- `error.rs`: provider-agnostic LLM error surface
- `retry.rs`: composable retry wrapper around any `LlmProvider`
- `manager.rs`: `LLMManager`, which lists configured providers and builds concrete provider instances from stored records
- `secret.rs`: host-bound API key encryption/decryption helpers
- `providers/openai_compatible.rs`: OpenAI-compatible provider factory and implementation
- `THIRD_PARTY_NOTICES.md`: vendored upstream provenance for LLM core files

## Responsibilities

- Keep `provider.rs`, `error.rs`, and `retry.rs` provider-agnostic
- Keep concrete integrations under `providers/`
- Keep provider selection and instantiation logic in `manager.rs`
- Keep API key handling out of provider records at rest; persisted secrets must remain encrypted

## Vendored Code

The LLM core is partially vendored from `nearai/ironclaw`.

For the exact provenance and local modifications to:
- `provider.rs`
- `error.rs`
- `retry.rs`

see `THIRD_PARTY_NOTICES.md` in this directory.

## Notes for Changes

- If you add a new concrete provider, place it under `providers/` and expose it through `mod.rs`
- If you change vendored files, update both their header comments and `THIRD_PARTY_NOTICES.md`
- Keep most tests inline with the implementation; reserve `crates/agent/tests/` for multi-module E2E scenarios
