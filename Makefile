.PHONY: fmt fmt-check lint test test-vm test-cli bench-vm docs-generate docs-check check ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-targets

test-vm:
	cargo test --test vm

test-cli:
	cargo test --test cli

bench-vm:
	cargo bench --bench vm

docs-generate:
	sh scripts/generate-language-support.sh

docs-check:
	sh scripts/docs-check.sh

check: fmt-check lint test docs-check

ci: check
