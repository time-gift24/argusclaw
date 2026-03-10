# Dev CLI for LLM Configuration and Manual Validation

## Goal

Add a development-only CLI surface for managing stored LLM providers and sending direct LLM requests for manual verification.

The CLI should only expose these commands when the `dev` feature is enabled. The production build should keep the current startup-only behavior.

## Scope

This change covers:

- a `dev` cargo feature in both `agent` and `cli`
- `LLMManager` dev-only passthrough methods for provider management and direct completion calls
- provider management commands in `cli`
- TOML import support for one or more providers
- a basic `llm complete` command for manual validation

This change does not cover:

- streaming CLI output
- multi-turn session files
- tool-calling workflows
- provider deletion
- non-TOML import formats

## Design

### Feature gating

- `crates/agent` gets a `dev` feature for dev-only helper APIs on top of the existing library surface.
- `crates/cli` gets a matching `dev` feature and forwards it to `agent/dev`.
- `clap` and `toml` are only compiled into `cli` when `dev` is enabled.

### CLI behavior

Without `dev`, `cli` keeps its current behavior: initialize tracing, open the database, run migrations, and start the app bootstrap path.

With `dev`, `cli` accepts optional subcommands:

- no subcommand: same startup behavior as today
- `provider ...`: manage stored provider configuration
- `llm ...`: send a one-shot completion request

### Commands

#### Provider commands

- `provider list`
- `provider get --id <id>`
- `provider upsert --id ... --display-name ... --kind openai-compatible --base-url ... --api-key ... --model ... [--default]`
- `provider import --file <path.toml>`
- `provider set-default --id <id>`
- `provider get-default`

`list`, `get`, and `get-default` must not print decrypted API keys.

#### LLM commands

- `llm complete --provider <id> --prompt "..."`
- `llm complete --default --prompt "..."`

The command builds a `CompletionRequest` with one `user` message and prints the returned text body.

### TOML import format

The import format is:

```toml
[[providers]]
id = "openai"
display_name = "OpenAI"
kind = "openai-compatible"
base_url = "https://api.openai.com/v1"
api_key = "sk-..."
model = "gpt-4o-mini"
is_default = true
```

The importer accepts one or more entries in the `providers` array and upserts them sequentially through `LLMManager`.

### Agent-layer passthrough

The repository remains private to `agent`.

`LLMManager` gets dev-only passthrough methods so the CLI can stay at the agent boundary:

- `upsert_provider`
- `import_providers`
- `get_provider_record`
- `get_default_provider_record`
- `set_default_provider`
- `complete_text`

`Agent` mirrors the same operations so the CLI can depend on `Agent` rather than reaching into storage directly.

### Database changes

The repository gains one new required capability:

- `set_default_provider(id)`

The SQLite implementation updates the default flag in a transaction:

1. verify the target provider exists
2. clear any existing default
3. mark the requested provider as default

### Error handling

- import should fail fast on malformed TOML or missing required fields
- `set-default` should return a clear not-found error when the provider id does not exist
- `llm complete` should surface provider lookup/configuration/request failures without hiding the cause

### Testing

- inline tests for TOML parsing and CLI argument parsing
- existing e2e tests in `crates/agent/tests/` extended to cover default selection updates
- optional lightweight CLI parser tests, no network calls
