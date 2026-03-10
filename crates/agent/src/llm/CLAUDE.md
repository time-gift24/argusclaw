# LLM Module Guide

- Keep `provider.rs`, `error.rs`, and `retry.rs` provider-agnostic. Provider-specific request fields belong in `providers/`.
- `manager.rs` owns provider lookup and instantiation from DB records. `Agent` should call `LLMManager`, not repositories directly.
- Thinking/reasoning is a shared capability:
  - non-streaming: `CompletionResponse.reasoning_content`
  - streaming: `LlmStreamEvent::ReasoningDelta`
  - capability detection: provider metadata/capabilities, not CLI heuristics
- OpenAI-compatible integrations may map vendor fields like `reasoning_content`, but that mapping stays inside `providers/openai_compatible.rs`.
- Persisted API keys must stay encrypted at rest; `secret.rs` is the boundary for host-bound encryption/decryption.
- Vendored provenance for `provider.rs`, `error.rs`, and `retry.rs` lives in `THIRD_PARTY_NOTICES.md`. If you change vendored files, update both the file header and the notice.
