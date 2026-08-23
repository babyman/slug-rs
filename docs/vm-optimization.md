# VM Optimization Plan

This document tracks planned improvements to the private Rust VM and bytecode.
It is an implementation roadmap, not a source-language specification or a
portable `.cslug` contract. The governing architecture choice is recorded in
[Adopt a measured private-bytecode optimization direction](decisions/2026-08-22-vm-bytecode-optimization-direction.md).

## Goals and invariants

Optimization must preserve these boundaries:

- invalid source produces `SourceError` and invalid bytecode or runtime faults
  produce `RuntimeError`, never a host panic;
- source locations and language call frames remain available on failures;
- mutable bindings captured by sibling or nested closures share identity;
- private bytecode remains independent of the versioned `.cslug` format; and
- each change is measured independently so its effect is not attributed to a
  simultaneous operand-model rewrite.

## Stage 0: establish measurements

- [ ] Add representative benchmarks for arithmetic and branches, calls,
  closures, `recur`, matching, deferred cleanup, and list/map operations.
- [ ] Record executed instruction count, elapsed time, instruction storage,
  `Value` cloning or reference-count traffic, and frame/local allocation.
- [ ] Keep correctness tests separate from performance thresholds; benchmarks
  inform architecture and do not make timing a flaky test requirement.

## Stage 1: borrow during dispatch

- [ ] Make instruction fetch return a borrowed instruction or borrowed opcode
  and source-location reference.
- [ ] Remove the unconditional `Instruction::clone` from the dispatch loop.
- [ ] Clone names, descriptors, and source locations only on paths that need
  owned data, especially error construction and global definition.
- [ ] Preserve all invalid-bytecode checks and existing runtime diagnostics.

Completion requires the VM and CLI suites plus a benchmark comparison showing
the change in instruction cloning and execution time.

## Stage 2: compact source and opcode metadata

- [ ] Intern source paths once per program or source table and identify them
  with `SourceId`.
- [ ] Store compact `SpanId` references with bytecode and resolve them to the
  public `SourceSpan` form only when producing a diagnostic.
- [ ] Move variable-size opcode data—global names, patterns, capture lists,
  schema fields, and struct field lists—into indexed chunk or program metadata
  pools.
- [ ] Evaluate a compressed per-chunk source map after the simple indexed form
  is measured.
- [ ] Keep Rust-owned metadata and layout out of the `.cslug` contract.

Completion requires unchanged source-located error behavior, malformed-index
tests for each metadata pool, and before/after instruction-size measurements.

## Stage 3: direct ordinary locals and promoted captures

- [ ] Represent an ordinary frame local as a direct `Value`.
- [ ] Promote a local to a shared binding cell when a closure captures it.
- [ ] Make local reads and writes transparently handle direct and promoted
  storage.
- [ ] Emit captures only for outer bindings referenced by a function body,
  using lazy capture creation or explicit free-variable analysis.
- [ ] Preserve capture identity when `recur` replaces the active function's
  parameter and local values.

Focused coverage must include uncaptured mutable locals, capture before and
after assignment, sibling closures, captures through intermediate functions,
captured parameters, escaping captures across `recur`, and deferred closures.

## Stage 4: compact executable representation

- [ ] Select field widths and instruction formats from measured program limits;
  do not make Rust enum layout or host pointer width part of the format.
- [ ] Keep regular operations for loads, moves, arithmetic, comparison, jumps,
  and returns.
- [ ] Keep calls, closure creation, collection construction, matching, cleanup,
  throwing, and recurrence as medium-grained semantic operations where that
  avoids repeated dispatch or duplicated validation.
- [ ] Add a fused superinstruction only when a benchmark identifies a stable
  hot sequence and the added verifier/compiler complexity is justified.

Compact encoding is independent of whether expression temporaries use an
operand stack or frame registers.

## Stage 5: stack-versus-register decision gate

Do not convert to a register VM based only on the general performance of Lua or
another language. Build a prototype or lowering experiment after the earlier
stages and compare it with the optimized stack VM using the same programs.

A register proposal must report:

- dynamic instruction-count reduction;
- wall-clock and dispatch-time changes;
- executable and metadata size;
- additional `Value` clone, move, and drop traffic;
- temporary-slot allocation and control-flow-join complexity;
- call, multi-binding match, capture, cleanup, and `recur` behavior; and
- verifier changes needed for register bounds and definite initialization.

Adopting a register VM requires a new decision record. Until then, the operand
stack remains the implementation strategy, not a compatibility promise.
