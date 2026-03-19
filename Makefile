.PHONY: help build build-prod build-dev install clean test run-prod run-dev clippy fmt tauri-dev tauri-build desktop-dev desktop-build

help:
	@printf '%s\n' \
		'Available targets:' \
		'  build            Build both CLI binaries' \
		'  build-prod       Build arguswing (production CLI)' \
		'  build-dev        Build arguswing-dev (development CLI)' \
		'  test             Run tests with all features' \
		'  run-prod         Run arguswing provider list' \
		'  run-dev          Run arguswing-dev --help' \
		'  clean            Clean build artifacts' \
		'  install          Install required tools (sqlx-cli, prek)' \
		'  clippy           Run clippy linter' \
		'  fmt              Format code' \
		'  tauri-dev        Run Tauri desktop app in dev mode' \
		'  tauri-build      Build Tauri desktop app for production' \
		'  desktop-dev      Alias for tauri-dev' \
		'  desktop-build    Alias for tauri-build'

build: build-prod build-dev

build-prod:
	cargo build --bin arguswing

build-dev:
	cargo build --bin arguswing-dev --features dev

test:
	cargo test -p cli --all-features

run-prod:
	cargo run --bin arguswing -- provider list

run-dev:
	cargo run --bin arguswing-dev --features dev -- --help

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
	cd crates/desktop && pnpm tauri dev

# Build Tauri desktop app for production
tauri-build:
	cd crates/desktop && pnpm tauri build

# Aliases for desktop development
desktop-dev: tauri-dev
desktop-build: tauri-build
