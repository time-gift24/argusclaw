.PHONY: help build-dev-cli

help:
	@printf '%s\n' \
		'Available targets:' \
		'  build-dev-cli    Build the cli crate with the dev feature enabled'

build-dev-cli:
	cargo build -p cli --features dev
