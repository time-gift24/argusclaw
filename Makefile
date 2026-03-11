.PHONY: help build-dev-cli build install

help:
	@printf '%s\n' \
        'Available targets:' \
        '  build-dev-cli    Build the cli crate with the dev feature enabled' \
        '  build            Build the production CLI (argusclaw)' \
        '  install          Install argusclaw to /usr/local/bin'

build-dev-cli:
	cargo build -p cli --features dev

build:
	cargo build -p cli --release

install: build
	cargo install --path crates/cli --bin cli --root /usr/local --force
