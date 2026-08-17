# Architecture

Slug is a clean-room Rust implementation of the Slug language. It currently
implements a small source subset through a checked bytecode virtual machine.

## Ownership

| Area | Owner | Responsibility |
|---|---|---|
| Source front end | `src/source.rs` | Lexing, parsing, and source-to-bytecode compilation. |
| Bytecode | `src/bytecode.rs` | Internal instruction and program representation. |
| Runtime values | `src/value.rs` | Dynamic language values and operations. |
| Execution | `src/vm.rs` | Frames, evaluation, and checked runtime failures. |
| CLI | `src/main.rs` | Process boundary and public error presentation. |

## Invariants

- Bytecode is an internal compiler-to-VM boundary, not a serialized format or
  compatibility promise.
- Invalid source is reported as `SourceError`; runtime failures are reported as
  `RuntimeError`. A Slug program must not expose a host panic as its diagnostic.
- Source spans and call frames remain attached to runtime failures whenever the
  information is available.
- Language semantics belong in `docs/language/`, not only in implementation
  code.
