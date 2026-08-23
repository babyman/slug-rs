# Architecture

Slug is a clean-room Rust implementation of the Slug language. It currently
implements a small source subset through a checked bytecode virtual machine.

## Ownership

| Area | Owner | Responsibility |
|---|---|---|
| Source front end | `src/source/` | Source façade, AST, lexer, parser, compiler, and lexical state. |
| Private bytecode | `src/bytecode.rs` | Internal instruction and program representation. |
| Compiled artifacts | `docs/compiled-artifacts.md` | Portable `.cslug` contract; implementation pending. |
| Native extensions | `docs/native-abi.md` | Opaque host calls, values, resources, threading, and future module ABI. |
| Runtime values | `src/value.rs` | Dynamic language values and operations. |
| Execution | `src/vm/` | VM dispatch, checked errors, cleanup unwinding, and value operations. |
| CLI | `src/main.rs` | Process boundary and public error presentation. |

## Invariants

- `Program`, `Chunk`, `Instruction`, and `Op` are an internal compiler-to-VM
  boundary, not a serialized format or compatibility promise.
- `.cslug` is the future portable compiled-module format.  It is a distinct,
  versioned contract and must not serialize private bytecode directly.
- Native extensions use an opaque call and resource boundary. They must not
  expose runtime value layouts, tasks, nurseries, or scheduler operations.
- Invalid source is reported as `SourceError`; runtime failures are reported as
  `RuntimeError`. A Slug program must not expose a host panic as its diagnostic.
- Source spans and call frames remain attached to runtime failures whenever the
  information is available.
- Language semantics belong in `docs/language/`, not only in implementation
  code.

## VM and bytecode direction

The current operand-stack VM remains the implementation baseline while known
instruction cloning, metadata, and local-storage costs are removed and
measured. Private bytecode favors a small, regular core with medium-grained
semantic operations for calls, closures, collections, matching, cleanup,
throwing, and recurrence. Variable-size descriptors belong in indexed metadata
pools rather than executable instructions.

The staged work and the evidence required before reconsidering a register VM
are defined in [VM Optimization Plan](vm-optimization.md). The durable choice is
recorded in [Adopt a measured private-bytecode optimization direction](decisions/2026-08-22-vm-bytecode-optimization-direction.md).
