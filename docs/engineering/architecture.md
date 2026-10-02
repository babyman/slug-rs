# Architecture

Slug is a clean-room Rust implementation of the Slug language. It currently
implements a small source subset through a checked bytecode virtual machine.

## Navigation and ownership

| Area | Owner | Responsibility |
|---|---|---|
| VM and bytecode | `crates/slug-vm/` | Owns dynamic values, private bytecode, checked execution, native registration contracts, and VM host callbacks. Its in-process Rust boundary is explicitly unstable. |
| Import contracts | `crates/slug-loader/` | Defines logical import requests, opaque source identities and activation leases, checked load failures, and resolver-owned activation transactions. It has no VM or desktop-policy dependency. |
| Source façade and interactive compilation | `crates/slug-frontend/src/source/{mod,interactive}.rs` | Public compile/error boundary plus source-readiness and persistent compiler snapshots for interactive cells. |
| Source syntax | `crates/slug-frontend/src/source/syntax/{lexer,parser,ast}.rs` | Turns source text into the private AST. Syntax does not depend on semantic analysis or bytecode. |
| Source semantics | `crates/slug-frontend/src/source/semantics/{typecheck,environment,semantic}.rs` | Resolves bindings, imports, annotations, inferred types, and compiler-facing semantic snapshots from the AST. |
| Source lowering | `crates/slug-frontend/src/source/lowering/{compiler,state}.rs` | Consumes AST plus semantic analysis and constructs the public-but-unstable bytecode representation. |
| In-process bytecode | `crates/slug-vm/src/bytecode/` | Public but unstable Rust instruction/program representation: metadata, builder operations, chunks, and installation/validation. |
| Compiled artifacts | `docs/reference/compiled-artifacts.md` | Portable `.cslug` contract; implementation pending. |
| Modules and graph caches | `crates/slug-frontend/src/module.rs` | Owns source-module compilation, semantic snapshots, isolated initialization, live exports, and cycle detection. |
| Desktop imports and Clutches | `crates/slug-desktop-loader/` | Resolves importer-relative, project-root, library-root, and Clutch modules; owns desktop-native activation and shutdown. |
| Restricted imports | `crates/slug-nil-loader/` and `crates/slug-restricted-host/` | Provides a deny-all resolver and a thin in-memory host convenience façade. |
| Native extensions | `docs/reference/native-abi.md` | Opaque host calls, values, resources, threading, and future module ABI. |
| Runtime values and collections | `crates/slug-vm/src/{value,collections}.rs` | Dynamic values, channels, tasks, and persistent collection storage remain coupled by runtime lifecycle. |
| Execution | `crates/slug-vm/src/vm/` | One VM owner for installation, dispatch, and polling; focused modules own frames/local storage, checked operand-stack access, errors, cleanup unwinding, operations, scheduler state, timers, and progress. |
| CLI and server | `crates/slug/src/{main,server,interactive}/` | Owns desktop configuration assembly, entry lookup, VM/frontend composition, public error presentation, and `slug --server`. |
| Interactive server | `crates/slug/src/interactive/` | Versioned NDJSON protocol, session ownership, source-cell lifecycle, and event projection over a VM. |
| Terminal REPL | `crates/slug-repl/src/main.rs` | Terminal input/editing and transport to `slug --server`; it does not embed VM behavior. |

The current source files are intentionally a smaller set than the eventual
stage-oriented directories described in the [agentic refactoring plan](../planning/completed/agentic-development-refactoring.md).
Until a responsibility is moved, this table is the ownership map rather than a
claim that a future directory already exists.

## Dependency direction

```text
source text -> frontend (syntax -> semantics -> lowering) -> bytecode -> VM
                    |                                      ^
                    +-> module graph -> loader contracts ---+
                                      ^
                         desktop loader / nil loader

slug CLI and server -> frontend + desktop loader + VM
restricted host -> frontend + nil loader + VM
REPL -> slug --server
```

Syntax produces private ASTs. Semantic analysis consumes syntax and produces
analysis/snapshot data. Lowering consumes both and creates `Program`; it is the
only source stage that depends on bytecode. The VM may retain an opaque
module-identity string for runtime imports, but it does not depend on loader
types. Semantic analysis must not depend on VM execution or bytecode encoding
details. The module graph uses source compilation and installs resulting
programs through the VM; resolver-owned activation transactions keep desktop
native lifecycle details outside that graph.

## Invariants

- `Program`, `Chunk`, `Instruction`, and `Op` are a public but unstable
  in-process Rust compiler-to-VM boundary. Rust hosts and integration tests may
  construct them, but only a `Vm` may turn an owned `Program` into an immutable
  `InstalledProgram` for execution. Their layouts, variants, constructors, and
  semantics are not a stable Rust API or serialized format.
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
- Slug code and VM-owned state have one host-driving owner. Native producers
  enqueue restricted owned values and notify possible progress; they never
  enter the interpreter or resume a particular task.
- `Vm::poll` and `Vm::run_until_stalled` perform only immediately available
  work. `Vm::blocking_run` is the adapter that may wait for a notification or
  timer, preserving the convenient blocking API for the CLI.

The public `VmProgress::Stalled` result currently covers both an in-flight
execution waiting for external or scheduled progress and a VM with no active
host execution. The VM retains the internal state needed to distinguish native
ingress, timer, and quiescent conditions, but a separate public `Idle` result
is deferred until a long-lived embedding API needs that distinction.

## VM and bytecode direction

The current operand-stack VM remains the implementation baseline while known
instruction cloning, metadata, and local-storage costs are removed and
measured. Private bytecode favors a small, regular core with medium-grained
semantic operations for calls, closures, collections, matching, cleanup,
throwing, and recurrence. Variable-size descriptors belong in indexed metadata
pools rather than executable instructions.

The staged work and the evidence required before reconsidering a register VM
are defined in [VM Optimization Plan](../planning/vm-optimization.md). The durable choice is
recorded in [Adopt a measured private-bytecode optimization direction](../decisions/2026-08-22-vm-bytecode-optimization-direction.md).
