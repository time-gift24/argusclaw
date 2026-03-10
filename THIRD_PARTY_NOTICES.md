# Third-Party Notices

## nearai/ironclaw llm core

- Source repository: https://github.com/nearai/ironclaw
- Upstream commit: `bcef04b82108222c9041e733de459130badd4cd7`
- License: `MIT OR Apache-2.0`
- Imported files:
- `crates/agent/llm/provider.rs`
- `crates/agent/llm/error.rs`
- `crates/agent/llm/retry.rs`
- Local modifications:
- Reduced to ArgusClaw's provider-agnostic `crate::llm` core API.
- Adapted retry decoration to ArgusClaw's reduced `LlmError` surface and streaming setup methods.
- Excluded upstream provider implementations, routing, cache, registry, and session modules.
- Added explicit provenance headers and repository metadata for auditability.
