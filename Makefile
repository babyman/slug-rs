.PHONY: fmt fmt-check lint test test-vm test-cli test-ffi-prototype bench-vm docs-generate docs-check check ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --lib --bins --tests --features metrics

test-vm:
	cargo test --features metrics --test vm

test-cli:
	cargo test --features metrics --test cli

test-ffi-prototype:
	cargo test --features ffi-prototype --test ffi_prototype

bench-vm:
	cargo bench --bench vm --features metrics

docs-generate:
	sh scripts/generate-language-support.sh

docs-check:
	sh scripts/docs-check.sh

check: fmt-check lint test docs-check

ci: check
