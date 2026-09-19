# Testing

Tests are evidence for specified behavior. They do not create a language
guarantee when the language documents say otherwise.

| Test | Use for | Focused command |
|---|---|---|
| `crates/slug-vm/tests/vm.rs` | Bytecode execution, values, globals, closures, frames, scheduler behavior, and source spans. | `make test-vm` |
| `crates/slug-vm/tests/cli.rs` | Accepted source, printed output, exit status, and rendered diagnostics. | `make test-cli` |
| `crates/slug-vm/tests/module_loader.rs` | Import resolution, module initialization, exports, live bindings, and module-backed type information. | `cargo test -p slug-vm --features metrics --test module_loader` |
| `crates/slug-vm/tests/configuration.rs` | Immutable configuration loading, precedence, conversions, and source builtins. | `cargo test -p slug-vm --features metrics --test configuration` |
| `crates/slug-vm/tests/conformance_runner.rs` | Fixture-sidecar parsing and process-level success or failure execution. | `cargo test -p slug-vm --features metrics --test conformance_runner` |
| `crates/slug-vm/tests/conformance_metadata.rs` | Rejection of malformed or incompatible fixture metadata. | `cargo test -p slug-vm --features metrics --test conformance_metadata` |
| `crates/slug-vm/tests/legacy_syntax_conformance.rs` | The repository's schema-1 fixtures in `tests/conformance/legacy-syntax/`. | `cargo test -p slug-vm --features metrics --test legacy_syntax_conformance` |
| `crates/slug-server/tests/interactive_server.rs` | Server protocol lifecycle, sessions, output events, and root execution. | `cargo test -p slug-server --features metrics --test interactive_server` |
| `crates/slug-repl/tests/interactive_repl.rs` | Terminal client's process transport, prompts, and diagnostic rendering. | `cargo test -p slug-repl --features metrics --test interactive_repl` |

The VM and CLI targets are common loops, so Make exposes them directly. Run the
listed `cargo test --test …` command for the remaining focused integration
boundaries; `make test` runs all of them. The corresponding Make aliases are
`test-frontend`, `test-modules`, `test-server`, and `test-repl`.

The stable `vm.rs` and `cli.rs` integration-test targets are facades, not one
undifferentiated behavior bucket. Their child modules are the first place to
add a regression when one applies: VM tests are grouped around bytecode,
calls/native functions, collections, concurrency, host lifecycle, and runtime
metrics/structural validation; CLI tests are grouped around basics, language
core, modules, types, patterns/cleanup, concurrency, filesystem, standard
input, and diagnostics. Keep the facade target name and existing assertions
stable while reorganizing tests.

## Feature matrix

`make check` validates the default scheduler runtime with metrics and invokes
`make slim` for every supported no-default-features configuration. `make slim`
runs tests and strict Clippy both without extra features and with `metrics`.
Use `cargo test -p slug-vm --no-default-features --test vm` for a focused slim VM loop.

Add a regression test with every behavior change. Error behavior must assert a
Slug error or CLI diagnostic rather than merely proving that execution did not
panic. Source syntax and user-visible behavior require a CLI test even when a
VM test covers the same execution path.
