# Agentic Development Refactoring Plan

This is a dependency-ordered organizational refactoring plan. It must not
intentionally change Slug source semantics, bytecode behavior, runtime
behavior, public Rust exports, diagnostics, or performance.

## Outcome

A contributor making a focused change can identify the owning subsystem, its
invariants, its extension point, and the narrowest regression command.

```text
source -> syntax -> semantic analysis -> lowering -> bytecode -> VM
                                  |                       |
                          module resolution          runtime values

CLI -> VM
REPL -> server -> VM
embedded Rust hosts -> VM
```

Language documents and the public-but-unstable in-process bytecode boundary
remain authoritative. This plan does not alter the portable `.cslug` contract.

## Invariants

- Preserve crate-level `slug_vm` exports unless a separate API change is
  approved.
- Keep `Program`, `Chunk`, `Instruction`, and `Op` as the compiler-to-VM
  boundary; do not make private bytecode a serialized format.
- Keep source stages one-way: syntax produces ASTs, semantic analysis consumes
  syntax, and lowering consumes analysis to emit bytecode. Semantics must not
  depend on bytecode implementation details.
- Keep VM dispatch and execution orchestration cohesive; do not split it
  mechanically by opcode.
- Preserve one host-driving owner of VM state. Native producers enqueue values
  but never execute Slug code.
- Preserve checked `SourceError` and `RuntimeError` failures; no host panic is
  an acceptable user-visible outcome.
- A move-only change preserves test assertions. If an assertion changes, split
  the behavior change into its own task.
- Prefer modules and directories to new crates. A new crate needs an
  independent API/dependency reason outside this plan.

## Implementation status

Tasks 0 and 1 were completed on 2026-09-19 as a documentation-only baseline.
Task 2 reorganized regression-test facades without changing assertions. Task 3
split private bytecode ownership across metadata, operations, chunks, and
program installation/validation; `make bench-vm` and `make check` passed after
the relocation. These slices make no source-semantic, public-export,
diagnostic, or bytecode-format change. Task 4 made the existing syntax,
semantic, lowering, and interactive orchestration boundaries visible without
splitting cohesive analysis or lowering algorithms. Task 5 began by moving the
shared frame representation and local-slot storage behind a private VM module.
Task 6 confirmed that module loading remains the owner of its compiler/native
resource lifecycle, dynamic values remain coupled to channels and tasks, and
native producers remain an ingress-only boundary; no directory split is
justified for those representations yet.

## 0. Establish the baseline and guardrails

**Goal:** Make later moves reviewable as behavior-preserving changes.

- [x] Record the current module tree, public exports, integration test targets,
  and dependency hotspots in the tracking issue or first implementation PR.
- [x] Record permitted dependency directions, including VM use of source
  semantic metadata and source lowering's use of bytecode.
- [x] Run and retain a clean `make check` baseline.
- [ ] For VM hot-path moves, record a `make bench-vm` measurement before and
  after the move, without adding timing thresholds.
- [ ] Split any semantic, API, performance, or diagnostic change found during
  a move into an independent task.

**Done when:** reviewers can distinguish relocation-only diffs from behavior
changes and know the appropriate validation gate.

## 1. Publish the repository navigation layer

**Goal:** Give contributors ownership, invariant, and test-selection answers
without moving production code.

- [x] Update `docs/engineering/architecture.md` with the actual source,
  semantic, lowering, bytecode, VM, module, native, CLI, server, and REPL
  relationships shown above.
- [x] Keep the map navigational and link to detailed documents rather than
  duplicating them.
- [x] Add concise local `AGENTS.md` files in `crates/slug-vm/src/source/` and
  `crates/slug-vm/src/vm/`.
- [x] Add local guidance in `crates/slug-server/src/interactive/` for session
  ownership, protocol compatibility, and server-specific tests.
- [ ] Add module/native guidance only after their final directory boundary is
  chosen; do not duplicate the root guidance.
- [x] Add focused Make aliases for VM, frontend, modules, server, and REPL.
  Each must invoke existing narrow test/lint commands.
- [ ] Defer `check-types` until its target is independently scoped and cheap.

**Validate:** `git diff --check`, `make docs-check`, and every new Make target.

**Done when:** a developer can find a subsystem's focused command and
invariants without reading the root implementation files.

## 2. Organize regression tests by behavior

**Goal:** Make the primary regression home visible before production-code
reorganization.

- [x] Retain `crates/slug-vm/tests/vm.rs` and `crates/slug-vm/tests/cli.rs` as
  stable Cargo integration-test facades.
- [x] Group child modules by observable behavior, not implementation history.
- [x] Use VM groups for bytecode, calls/closures, collections, cleanup/errors,
  and concurrency; use CLI groups for syntax/bindings, functions, modules,
  types, patterns, concurrency, and diagnostics where existing tests fit.
- [x] Keep dedicated integration targets for module loading, configuration,
  conformance, FFI, server, and REPL behavior.
- [x] Move tests without duplication and preserve names/assertions initially.
- [x] Update `docs/engineering/testing.md` and local guidance with final
  locations and commands.

**Validate:** the affected test target after each move, then `make check`.

**Done when:** every architecture-map subsystem has an obvious primary test
home and existing `cargo test --test` names remain stable.

## 3. Establish a bytecode module boundary

**Goal:** Turn private bytecode into cohesive modules without changing the
crate-level exported surface.

```text
bytecode/
  mod.rs        # ownership and crate-level re-exports
  op.rs         # executable operation definitions
  chunk.rs      # chunks, constants, executable layout
  program.rs    # construction and installation-facing shape
  metadata.rs   # captures, spans, patterns, schemas, signatures
  verify.rs     # checked structural validation, if cohesive
```

- [x] Map type/function ownership before creating files and preserve existing
  public terminology.
- [x] Move one cohesive cluster at a time with the narrowest visibility.
- [x] Preserve `lib.rs` re-exports and malformed-bytecode checked failures.
- [x] Keep compact instruction representation and performance-sensitive
  encoding with the owning execution representation.
- [x] Do not add serialization or blur the `.cslug` distinction.

**Validate:** `make test-vm`, malformed-bytecode coverage, `make bench-vm`
for hot-code moves, then `make check`.

**Done when:** bytecode types, metadata, and validation have one-sentence
responsibilities and lowering/VM can navigate their shared boundary directly.

## 4. Make source processing stages explicit

**Goal:** Organize `source/` around syntax, semantics, lowering, and
interactive orchestration while retaining its private crate boundary.

```text
source/
  mod.rs              # compile facade, errors, orchestration
  syntax/{lexer,parser,ast}.rs
  semantics/{environment,bindings,types,inference,narrowing,calls,diagnostics}.rs
  lowering/{expressions,statements,calls,patterns,closures}.rs
  interactive.rs
```

- [x] Keep the `source/` name at the crate boundary initially; it is
  established terminology and avoids a gratuitous rename.
- [x] Move lexer, parser, and AST first, with no algorithm change.
- [x] Extract semantic data/environment from the type checker before splitting
  analysis passes.
- [x] Split semantic analysis only by real responsibility: bindings,
  annotations/types, inference, narrowing, calls/generics, and diagnostics.
- [x] Extract lowering from `compiler.rs` only after semantic input APIs are
  explicit; do not put semantic decisions into bytecode emission.
- [x] Isolate interactive compilation/session state while retaining resolver
  and commit behavior.
- [x] Update source-local guidance once names settle.

**Validate:** `make test-cli`, module-loader tests for import/semantic
snapshots, server tests for interactive compilation, then `make check`.

**Done when:** the tree mirrors `syntax -> semantics -> lowering -> bytecode`
and source changes can be located by processing stage.

## 5. Extract VM responsibilities without fragmenting dispatch

**Goal:** Make runtime ownership discoverable while retaining one coherent
execution loop.

```text
vm/
  mod.rs          # Vm, installation, polling, dispatch, orchestration
  execution.rs    # only if a cohesive state-transition seam exists
  frames.rs
  stack.rs
  calls.rs
  globals.rs
  cleanup.rs
  errors.rs
  operations.rs
  scheduler.rs
  timers.rs
  progress.rs
```

- [x] Identify helper clusters in `vm/mod.rs` by data ownership/lifecycle, not
  by individual opcode arms.
- [x] Extract frames/stack only if checked-error and source-span paths stay
  straightforward.
- [ ] Extract call and global helpers only if closure capture and live binding
  behavior remain directly traceable.
- [ ] Keep cleanup/error unwinding explicit: `defer`, recovery, spans, and
  call-frame retention.
- [ ] Keep scheduler admission, task state, channels, wait registration,
  select cancellation, and timers visibly grouped.
- [ ] Verify native producers still only enqueue values and never mutate
  VM-owned state directly.
- [ ] Reject `vm/opcodes/*` fragmentation unless profiling and ownership show
  it improves the result.

**Validate:** `make test-vm`, focused slim-runtime VM coverage, relevant server
tests for retained interactive work, `make bench-vm` for hot-path moves, then
`make check`.

**Done when:** `vm/mod.rs` remains the execution owner while supporting runtime
responsibilities have focused modules, invariants, and test homes.

## 6. Review remaining runtime and module boundaries

**Goal:** Group remaining files only where completed work proves a stable
responsibility boundary.

- [x] Re-evaluate module loading, clutch discovery, native registration, FFI
  prototype support, dynamic values, collections, and configuration after
  Tasks 3--5.
- [ ] Create a directory only when it does not introduce a `source <-> vm` or
  `value <-> vm` dependency cycle.
- [ ] Keep coupled value/task/runtime-state representations together unless an
  extracted API has clear ownership and failure behavior.
- [ ] Add local guidance only at settled boundaries.
- [ ] Update architecture/testing maps with final paths.

**Validate:** every affected boundary's focused target, then `make check`.

**Done when:** every major top-level implementation area has a discoverable
owner, without a new crate created solely for folder organization.

## Execution and handoff

Tasks 0 and 1 may land together. Complete Task 2 before moving the matching
production subsystem. Complete Task 3 before Tasks 4 and 5 where otherwise the
bytecode boundary becomes unclear. Defer Task 6 until earlier moves reveal
actual dependency seams.

Each implementation PR should complete one checkbox cluster, name preserved
invariants and focused commands, and run `make check` before handoff. For a
documentation-only slice, use `git diff --check` and `make docs-check`. The
plan is complete when this document's Outcome holds for frontend, bytecode,
VM, modules/native integration, server, and REPL work.
