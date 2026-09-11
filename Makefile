.PHONY: fmt fmt-check lint test slim test-vm test-cli test-ffi-prototype stage-native-clutches bench-vm docs-generate docs-check check ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --lib --bins --tests --features metrics

slim:
	cargo test --no-default-features
	cargo test --no-default-features --features metrics
	cargo clippy --all-targets --no-default-features -- -D warnings
	cargo clippy --all-targets --no-default-features --features metrics -- -D warnings

test-vm:
	cargo test --features metrics --test vm

test-cli:
	cargo test --features metrics --test cli

test-ffi-prototype:
	cargo test --test ffi_prototype

stage-native-clutches:
	sh scripts/stage-native-clutches.sh

bench-vm:
	cargo bench --bench vm --features metrics

docs-generate:
	sh scripts/generate-language-support.sh

docs-check:
	sh scripts/docs-check.sh

check: fmt-check lint test slim docs-check

ci: check
