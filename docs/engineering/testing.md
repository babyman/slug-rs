# Testing

Tests are evidence for specified behavior. They do not create a language
guarantee when the language documents say otherwise.

| Test | Use for | Focused command |
|---|---|---|
| `tests/vm.rs` | Bytecode execution, values, globals, closures, frames, scheduler behavior, and source spans. | `make test-vm` |
| `tests/cli.rs` | Accepted source, printed output, exit status, and rendered diagnostics. | `make test-cli` |
| `tests/module_loader.rs` | Import resolution, module initialization, exports, live bindings, and module-backed type information. | `cargo test --features metrics --test module_loader` |
| `tests/configuration.rs` | Immutable configuration loading, precedence, conversions, and source builtins. | `cargo test --features metrics --test configuration` |
| `tests/conformance_runner.rs` | Fixture-sidecar parsing and process-level success or failure execution. | `cargo test --features metrics --test conformance_runner` |
| `tests/conformance_metadata.rs` | Rejection of malformed or incompatible fixture metadata. | `cargo test --features metrics --test conformance_metadata` |
| `tests/legacy_syntax_conformance.rs` | The repository's schema-1 fixtures in `tests/conformance/legacy-syntax/`. | `cargo test --features metrics --test legacy_syntax_conformance` |

The VM and CLI targets are common loops, so Make exposes them directly. Run the
listed `cargo test --test …` command for the remaining focused integration
boundaries; `make test` runs all of them.

Add a regression test with every behavior change. Error behavior must assert a
Slug error or CLI diagnostic rather than merely proving that execution did not
panic. Source syntax and user-visible behavior require a CLI test even when a
VM test covers the same execution path.
