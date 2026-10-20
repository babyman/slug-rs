.PHONY: fmt fmt-check lint test slim test-vm test-cli test-frontend test-loader test-nil-loader test-desktop-loader test-slug test-modules test-server test-repl test-native test-clutches bench-native-scalar stage-native-clutches test-filesystem-clutch bench-vm bench-source bench-ab measure-vm-memory docs-generate docs-check check ci

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
	cargo test --workspace --lib --bins --tests --features metrics

slim:
	cargo test --workspace --no-default-features
	cargo test --workspace --no-default-features --features metrics
	cargo clippy --workspace --all-targets --no-default-features -- -D warnings
	cargo clippy --workspace --all-targets --no-default-features --features metrics -- -D warnings

test-vm:
	cargo test -p slug-vm --features metrics --test vm

test-cli:
	cargo test -p slug --features metrics --test cli

# Focused subsystem aliases. Keep their integration-test target names stable.
test-frontend:
	cargo test -p slug-frontend --all-targets

test-loader:
	cargo test -p slug-loader --all-targets

test-nil-loader:
	cargo test -p slug-nil-loader --all-targets

test-desktop-loader:
	cargo test -p slug-desktop-loader --all-targets

test-slug:
	cargo test -p slug --all-targets

test-modules:
	cargo test -p slug --features metrics --test module_loader

test-server:
	cargo test -p slug --features metrics --test interactive_server

test-repl:
	cargo test -p slug-repl --features metrics --test interactive_repl

test-native:
	cargo test -p slug --test typed_native
	cargo test -p slug --test typed_sqlite
	cargo test -p slug --test typed_core
	cargo test -p slug-native-manifest

# The clutches' own tests, written in Slug and run by `slug test` with this
# repository as SLUG_HOME.
test-clutches:
	cargo build -p slug
	SLUG_HOME=$(CURDIR) target/debug/slug --allow-unexported test clutch/slug.core.clutch

bench-native-scalar:
	sh scripts/benchmark-native-scalar.sh

stage-native-clutches:
	sh scripts/stage-native-clutches.sh

# Stages only the typed filesystem clutch and runs its CLI tests.
test-filesystem-clutch:
	sh scripts/stage-native-clutches.sh slug.io.fs.clutch
	cargo test -p slug --features metrics --test cli filesystem

bench-vm:
	cargo bench -p slug-frontend --bench vm --features metrics

bench-source:
	cargo build --release -p slug --bin slug
	cargo bench -p slug-frontend --bench source

# Interleaved A/B timing; BASE defaults to master, CANDIDATE to the working tree.
bench-ab:
	python3 -I scripts/bench-ab.py $(or $(BASE),master) $(CANDIDATE) $(BENCH_AB_ARGS)

measure-vm-memory:
	sh scripts/measure-vm-memory.sh

docs-generate:
	sh scripts/generate-language-support.sh

docs-check:
	sh scripts/docs-check.sh

check: fmt-check lint test slim docs-check test-clutches

ci: check
