# AGENTS.md

Slug is a clean-room Rust implementation of the Slug language. It currently
contains a checked bytecode VM and a deliberately small source-language subset.
Build the language foundation before adding compatibility layers or speculative
abstractions.

## Repository map

```
src/        Runtime, bytecode, dynamic values, and source compiler
tests/      VM, CLI, module-loader, configuration, and conformance integration tests
docs/       Architecture, development policy, and canonical language documents
.agents/    Agent workflows and decision-record guidance
```

Read [README.md](README.md) before changing runtime behavior. For a source
language change, read the relevant file in `docs/language/` and
`.agents/workflows/language-change.md` before editing.

## Commands

Rust is installed through `rustup`; use the repository's `Cargo.lock`.

```sh
make fmt          # Format Rust sources
make fmt-check    # Verify formatting without editing
make lint         # Run Clippy with warnings treated as errors
make test         # Run unit and integration tests
make test-vm      # Run bytecode VM tests only
make test-cli     # Run public CLI tests only
make docs-generate # Regenerate the implementation support matrix
make docs-check   # Verify documentation inventory and generated output
make check        # Run format, lint, and the full test suite
cargo run -- --help
```

Run the narrowest test that proves a change while iterating. Before handing off
a Rust change, run `make check`. For documentation-only changes, run
`git diff --check` and validate every command or file reference you changed.

## Test routing

| Boundary | Test | Focused command |
|---|---|---|
| Private bytecode and VM/runtime behavior | `tests/vm.rs` | `make test-vm` |
| Source syntax, CLI output, and diagnostics | `tests/cli.rs` | `make test-cli` |
| Imports, modules, exports, and live bindings | `tests/module_loader.rs` | `cargo test --features metrics --test module_loader` |
| Configuration loading and `cfg`-related behavior | `tests/configuration.rs` | `cargo test --features metrics --test configuration` |
| Fixture execution behavior | `tests/conformance_runner.rs` | `cargo test --features metrics --test conformance_runner` |
| Fixture-sidecar validation | `tests/conformance_metadata.rs` | `cargo test --features metrics --test conformance_metadata` |
| Repository legacy-syntax fixtures | `tests/legacy_syntax_conformance.rs` | `cargo test --features metrics --test legacy_syntax_conformance` |

`make test` runs the full unit, binary, and integration suite. See
[`docs/engineering/testing.md`](docs/engineering/testing.md) for each layer's
scope and regression expectations.

## Tooling

When the IntelliJ IDEA MCP server is enabled, use it for repository navigation,
symbol-aware search, diagnostics, refactoring, and running IDE configurations.
Fall back to command-line tools when the server is unavailable or does not
support the required operation.

## Change rules

- Keep source-language semantics in `docs/language/`, not only in implementation
  code or tests.
- A syntax or semantic change must update the applicable specification,
  `docs/language/slug.ebnf`, and the README capability statement when it changes
  the implemented subset. Update focused design notes only when they own the
  affected feature.
- Prove bytecode behavior in `tests/vm.rs`. Prove source syntax, diagnostics,
  or observable output through `tests/cli.rs`.
- Keep bytecode internal. Do not introduce serialized opcode formats or expose
  opcode values as a language compatibility promise.
- Preserve checked failures: invalid source and runtime faults must produce
  `SourceError` or `RuntimeError`, never a host panic.
- Keep comments and public Rust documentation focused on non-obvious behavior,
  failure modes, ownership, or invariants.
- Do not edit `Cargo.lock` manually. Do not add dependencies unless they remove
  more owned complexity than they create.
- Do not change unrelated tests to make a refactor pass. If pre-existing tests
  fail, report the failure instead of hiding it.

## Design records

Create an agent note under `docs/decisions/` for a non-trivial decision that
changes syntax, semantics, runtime architecture, error behavior, or the
compatibility policy. Follow `docs/decisions/README.md`. Do not create a note
for mechanical edits or a local bug fix whose design is already specified.

## Working agreement

- Inspect existing code and tests before choosing an implementation.
- Keep commits scoped to one coherent prompt when a commit is requested. Use a
  Semantic Commit Message (`feat`, `fix`, `refactor`, or `chore`) with a
  50-character subject and a wrapped body.
- Append user-visible repository changes to `changelog.md` under `Unreleased`.
- Do not commit secrets, build artifacts, or local editor state.
