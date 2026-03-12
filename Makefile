.PHONY: help build-dev-cli install

help:
	@printf '%s\n' \
		'Available targets:' \
		'  build-dev-cli    Build the cli crate with the dev feature enabled' \
		'  install          Install required tools (sqlx-cli, prek)'

build-dev-cli:
	cargo build -p cli --features dev

install:
	cargo install sqlx-cli --no-default-features --features sqlite
	cargo install prek
	cargo install --locked cargo-deny && cargo deny init && cargo deny check
	prek install
