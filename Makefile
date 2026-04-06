.PHONY: help build install clean clippy fmt tauri-dev tauri-build desktop-dev desktop-build desktop-check server-dev server-check server-test

help:
	@printf '%s\n' \
		'Available targets:' \
		'  build            Build the workspace' \
		'  install          Install required tools (sqlx-cli, prek)' \
		'  clean            Clean build artifacts' \
		'  clippy           Run clippy linter' \
		'  fmt              Format code' \
		'  tauri-dev        Run Tauri desktop app in dev mode' \
		'  tauri-build      Build Tauri desktop app for production' \
		'  desktop-check    Check desktop crate only' \
		'  desktop-dev      Alias for tauri-dev' \
		'  desktop-build    Alias for tauri-build' \
		'  server-dev       Run the axum server in dev mode' \
		'  server-check     Check the server crate only' \
		'  server-test      Run server crate tests'

build:
	cargo build --workspace

clean:
	cargo clean

install-tools:
	cargo install sqlx-cli --no-default-features --features sqlite
	cargo install prek
	cargo install --locked cargo-deny && cargo deny init && cargo deny check
	prek install

# Run clippy linter
clippy:
	cargo clippy --workspace --all-targets

# Format code
fmt:
	cargo fmt --all
	cargo fmt --check --all

# Run Tauri desktop app in dev mode
tauri-dev:
	cd crates/desktop && pnpm install && pnpm tauri dev

# Build Tauri desktop app for production
tauri-build:
	cd crates/desktop && pnpm install && CI=true pnpm tauri build

# Aliases for desktop development
desktop-dev: tauri-dev
desktop-build: tauri-build

desktop-check:
	cargo check -p desktop

server-dev:
	cargo run -p argus-server

server-check:
	cargo check -p argus-server

server-test:
	cargo test -p argus-server
