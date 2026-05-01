.PHONY: help build install install-dev test lint clean

help:
	@echo "Available targets:"
	@echo "  build        Build release binary (cargo build --release)"
	@echo "  install      Install powercfg into ~/.cargo/bin"
	@echo "  install-dev  Install debug build into ~/.cargo/bin"
	@echo "  test         Run cargo test"
	@echo "  lint         Run clippy (-D warnings) and cargo fmt --check"
	@echo "  clean        Run cargo clean"

build:
	cargo build --release

install:
	cargo install --path .

install-dev:
	cargo install --path . --debug

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings && cargo fmt --check

clean:
	cargo clean
