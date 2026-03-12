.PHONY: help build build-prod build-dev install clean test run-prod run-dev

help:
	@printf '%s\n' \
		'Available targets:' \
		'  build            Build both CLI binaries' \
		'  build-prod       Build argusclaw (production CLI)' \
		'  build-dev        Build argusclaw-dev (development CLI)' \
		'  test             Run tests with all features' \
		'  run-prod         Run argusclaw provider list' \
		'  run-dev          Run argusclaw-dev --help' \
		'  clean            Clean build artifacts' \
		'  install          Install required tools (sqlx-cli, prek)'

build: build-prod build-dev

build-prod:
	cargo build --bin argusclaw

build-dev:
	cargo build --bin argusclaw-dev --features dev

test:
	cargo test -p cli --all-features

run-prod:
	cargo run --bin argusclaw -- provider list

run-dev:
	cargo run --bin argusclaw-dev --features dev -- --help

clean:
	cargo clean

install-tools:
	cargo install sqlx-cli --no-default-features --features sqlite
	cargo install prek
	cargo install --locked cargo-deny && cargo deny init && cargo deny check
	prek install
