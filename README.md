# ArgusClaw

A Rust-based Agent/LLM framework with extensible tool support and workflow orchestration.

## Features

- **Multi-provider LLM support**: OpenAI-compatible providers with extensible architecture
- **Tool system**: Built-in and custom tools for Agent/LLM operations
- **Workflow orchestration**: Complex multi-stage workflows with job management
- **SQLite storage**: Persistent storage for LLM providers, workflows, and execution history
- **Hook system**: Extensible hooks for turn execution lifecycle events
- **Secure secrets**: Host-bound API key encryption/decryption

## Built-in Tools

- **cookie_extractor**: Extract cookies from Chrome for a specific domain via CDP
  ```json
  {"action": "cookie_extractor", "cdpUrl": "...", "domain": "example.com"}
  ```
  See: [docs/tools/cookie_extractor.md](docs/tools/cookie_extractor.md)

## Project Structure

```
crates/
├── claw/              # Core library
├── desktop/           # Tauri + React desktop application
└── cli/               # Command-line interface
```

## Development

```bash
# Format code
cargo fmt

# Check for issues (zero warnings)
cargo clippy --all --benches --tests --examples --all-features

# Run tests
cargo test

# Run tests with SQLite integration
cargo test --features integration

# Run with debug logging
RUST_LOG=argusclaw=debug,claw=debug cargo run
```

## Documentation

- [Development Guide](CLAUDE.md) - Detailed development instructions and architecture
- [Tool Documentation](docs/tools/) - Built-in tools documentation
- [Implementation Plans](docs/plans/) - Feature implementation plans

## License

MIT OR Apache-2.0
