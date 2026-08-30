# VM Optimization Plan

This document tracks planned improvements to the private Rust VM and bytecode.
It is an implementation roadmap, not a source-language specification or a
portable `.cslug` contract. The governing architecture choice is recorded in
[Adopt a measured private-bytecode optimization direction](../decisions/2026-08-22-vm-bytecode-optimization-direction.md).

## Goals and invariants

Optimization must preserve these boundaries:

- invalid source produces `SourceError` and invalid bytecode or runtime faults
  produce `RuntimeError`, never a host panic;
- source locations and language call frames remain available on failures;
- mutable bindings captured by sibling or nested closures share identity;
- private bytecode remains independent of the versioned `.cslug` format; and
- each change is measured independently so its effect is not attributed to a
  simultaneous operand-model rewrite.

## Implementation sequence

Work through the stages in order. A stage may change private bytecode and its
direct VM tests, but it must not change source-language semantics or the
portable `.cslug` contract. Land one independently measurable change at a
time; do not combine representation changes with a register-VM experiment.

| Stage | Outcome | Entry evidence | Completion gate |
|---|---|---|---|
| 0 | Repeatable measurements | Representative programs and counters | Baseline report checked into the change description or an adjacent note |
| 1 | Borrowed dispatch | Stage 0 clone and time baseline | No unconditional instruction clone; diagnostics unchanged |
| 2 | Verified private bytecode | Existing malformed-bytecode coverage | Structural defects rejected before execution, with runtime checks retained |
| 3 | Compact metadata | Stage 1 measurements | Indexed metadata and unchanged source locations |
| 4 | Direct ordinary locals | Capture and `recur` regression matrix | Uncaptured locals avoid binding cells without changing capture identity |
| 5 | Scheduler scaling decision | Timer/select workload measurements | Keep the current queues or adopt a measured replacement |
| 6 | Compact executable representation | Program-size and dispatch measurements | Chosen encoding is justified by measured limits |
| 7 | Stack/register decision | Results from stages 0--6 | Separate decision record, if a register prototype wins |

Every implementation stage runs `make test-vm` during iteration and `make
check` before handoff. Benchmark numbers inform engineering decisions but do
not become timing-sensitive test assertions.

## Stage 0: establish measurements

- [x] Add representative benchmarks for arithmetic and branches, calls,
  closures, `recur`, matching, deferred cleanup, and list/map operations.
- [x] Record executed instruction count, elapsed time, whole-instruction
  cloning, and frame/local allocation. Add `Value` clone or reference-count
  traffic only when an optimization needs that more specific evidence.
- [ ] Keep correctness tests separate from performance thresholds; benchmarks
  inform architecture and do not make timing a flaky test requirement.

The benchmark harness is invoked with `make bench-vm`. It compiles each
representative source workload once, then reports elapsed time plus the
per-run VM counters accumulated across the benchmark: executed instructions,
whole-instruction clones, frame creation, and frame-local binding-cell
allocation. Keep additional measurements, such as `Value` clone or reference
count traffic, scoped to the representation change that needs them; do not
instrument every dynamic-value clone until a baseline identifies it as a
candidate cost.

### Initial baseline

The initial `make bench-vm` run establishes the following representation
counters per source-program invocation. These counts are stable regression
evidence; elapsed time remains machine-dependent and is intentionally not
recorded here.

| Workload | Instructions | Instruction clones | Frames | Local binding cells |
|---|---:|---:|---:|---:|
| arithmetic and branches | 2,825 | 2,825 | 2 | 2 |
| calls and closures | 32 | 32 | 3 | 2 |
| pattern matching | 28 | 28 | 2 | 3 |
| deferred cleanup | 26 | 26 | 3 | 1 |
| lists and maps | 35 | 35 | 1 | 0 |

The one-to-one instruction-clone ratio is the immediate justification for
Stage 1. Re-run the harness after that stage and compare counters and elapsed
time using the same workload set.

After the first Stage 1 slice, all five workloads report zero whole-instruction
clones. The arithmetic-and-branches workload completed 1,000 runs in about
286 ms on the baseline machine, compared with about 301 ms before the change.
This is directional evidence only; do not treat it as a portable timing
threshold. At this point, source spans were still cloned at the dispatch
boundary, so Stage 1 remained active until the diagnostic path could borrow or
otherwise defer spans.

The next Stage 1 slice borrows source spans for the common stack, arithmetic,
comparison, and control-flow operations. It records a separate source-span
clone counter for instructions that still require the owned-span path. The
arithmetic-and-branches workload now completes 1,000 runs in about 142 ms and
clones 611 source spans per run, down from 2,825 before borrowed-span dispatch.
Calls, `recur`, cleanup, and scheduler operations still use owned-span helper
APIs and are the remaining Stage 1 target.

List construction, map construction, and indexed access now share the borrowed
span path. The list-and-maps workload drops from 15 to 9 source-span clones per
invocation, while pattern matching drops from 9 to 8. These counts identify
`recur` and call binding as the next material sources of owned spans.

`recur` now borrows its span through argument extraction, expansion, and
parameter binding. The arithmetic-and-branches workload drops from 611 to 411
source-span clones per invocation and completes 1,000 runs in about 120 ms.
The remaining arithmetic clones are ordinary call, return, and declaration
operations rather than recurrence iterations.

Borrowed scope entry/exit and return handling reduce the arithmetic workload
from 411 to 5 source-span clones per invocation. The fast path preserves
cleanup-driven settlement when a scope exits or a function returns; it does
not assume those operations merely continue dispatch. The remaining clones are
function/declaration setup and ordinary calls, which require a separate
call-boundary refactor.

Global declarations, global lookup, closure construction, and ordinary calls
now use the borrowed-span path. An ordinary closure call clones its span only
to retain the call site in a durable call frame. The arithmetic workload now
has two source-span clones per invocation and completes 1,000 runs in about
100 ms across repeated warm runs. Spread, pipeline, import, cleanup, and
scheduler operations remain on owned-span paths.

Spread calls, selected overload calls, and pipeline calls now also retain a
borrowed span through argument expansion and overload binding. They clone only
when a selected closure or cross-module closure creates a durable call frame;
native call failures construct an owned diagnostic only on the error path.
Imports, cleanup, and scheduler operations remain for the Stage 1 ownership
audit.

Imports, list spreads, and deferred-action registration now borrow their spans
as well. The deferred-cleanup benchmark drops from three to two source-span
clones per invocation; the remaining clones are call-frame diagnostics and
other owned scheduler paths.

`select` now borrows its span while validating cases, resolving ready/default
cases, and constructing its registrations. It clones the span only after no
case can settle immediately, when the wait set is stored as a durable
`Suspension::Select`; resumed and closed-send errors therefore retain the
original source location.

Task spawning and explicit nursery setup now likewise borrow their spans while
validating operands and limits. They clone only when creating the task body
frame, which may outlive the instruction and must retain its call-site
diagnostics.

Struct construction, slicing, and match execution now borrow spans through
their normal path. A thrown value becomes a durable `RuntimeError`, so `throw`
clones its span only while constructing that error.

Interpolation, captures, global and module-metadata updates, overload setup,
and `select` handler application now complete the borrowed dispatch path. The
final benchmark ownership counts per invocation are: arithmetic and branches
1, calls and closures 2, pattern matching 1, deferred cleanup 1, and lists
and maps 0. The remaining clones are intentional durable owners: closure or
task call frames, suspended selects, and constructed runtime errors.

### Remaining Stage 1 execution plan

Finish Stage 1 in the following narrow slices. Re-run the focused VM and CLI
tests after each slice, then compare the affected benchmark workloads before
starting the next one.

1. **Variable-shape calls.** Thread borrowed spans through spread calls,
   pipeline calls, and imports, including argument expansion and native-call
   errors. Clone a span only when a closure frame must retain its call site or
   an error/suspended operation must own it. Cover positional, named, default,
   variadic, spread, selected-overload, and pipeline call paths.
2. **Cleanup and scheduling.** Borrow spans for `defer`, cleanup execution,
   task creation, and immediately-ready `select` cases. When a `select` or
   timed wait actually suspends, clone the span into its durable suspension
   state; that ownership boundary is intentional and should remain visible in
   the metrics. Preserve cancellation and winner-removes-losers behavior.
3. **Residual dispatch audit.** Classify every opcode still using the
   owned-span fallback. Move ordinary successful execution to the borrowed
   path, or document the durable owner that requires a clone. Add a
   per-opcode or per-category measurement only if the aggregate counter no
   longer identifies the remaining work clearly.
4. **Stage 1 closeout.** Capture the final benchmark counters beside the
   initial baseline and record the permitted clone boundaries: runtime-error
   construction, call-frame diagnostics, and suspended runtime state. Run
   `make check` before declaring the stage complete.

The immediate implementation slice is variable-shape calls. Its acceptance
criteria are unchanged source locations and language frames for failed calls,
unchanged overload selection and argument binding, and fewer source-span
clones in the calls-and-closures workload. Do not start bytecode verification
or metadata pooling until this borrowed-dispatch audit is complete; otherwise
the measurements will conflate independent representation changes.

## Stage 1: borrow during dispatch

- [x] Make instruction fetch return a borrowed instruction and borrowed opcode.
- [x] Remove the unconditional `Instruction::clone` from the dispatch loop.
- [x] Clone names, descriptors, and source locations only on paths that need
  owned data, especially error construction and global definition.
- [x] Preserve all invalid-bytecode checks and existing runtime diagnostics.

Completion requires the VM and CLI suites plus a benchmark comparison showing
the change in instruction cloning and execution time.

## Next-stage decision gates

Once Stage 1 closes, begin Stage 2 with structural checks that are independent
of execution: chunk and constant references, jump targets, declared
arity/local consistency, local slots, and statically known metadata indices.
Add stack-height analysis only after those simple checks have focused tests;
keep runtime checks because private bytecode can still be constructed manually.

After the verifier is established, Stage 3 may introduce indexed source and
operand metadata. Stage 4's direct-local representation must follow its
capture/recur regression matrix, not benchmark pressure alone. Stage 5 is a
measurement decision, not a presumed queue replacement. Stages 6 and 7 remain
deferred until their stated size and dispatch evidence exists.

## Stage 2: verify private bytecode before execution

- [ ] Add a verifier invoked at the public VM entry boundary before a frame is
  created.
- [ ] Validate chunk and constant references, jump targets, local-slot bounds,
  declared arity/local consistency, and metadata-pool indices that can be
  checked without executing values.
- [ ] Track a conservative operand-stack height through control flow and
  reject statically provable underflow or incompatible control-flow joins.
- [ ] Keep the dispatch-time checks for all dynamic and verifier-independent
  failures: manually constructed private bytecode remains untrusted.
- [ ] Add table-driven malformed-program tests plus property or fuzz coverage
  asserting that arbitrary private bytecode returns `RuntimeError` rather than
  panicking.

Completion requires malformed bytecode to fail deterministically before
observable partial execution whenever the defect is statically knowable, and
all existing source-located runtime diagnostics to remain intact.

## Stage 3: compact source and opcode metadata

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

## Stage 4: direct ordinary locals and promoted captures

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

## Stage 5: measure scheduler scaling before replacing queues

- [ ] Extend the benchmark corpus with many timers, many `select` cases, and
  cancellation of suspended waits.
- [ ] Record timer registration, next-deadline lookup, wakeup, and loser-removal
  costs separately from ordinary VM dispatch.
- [ ] Retain the current vector-backed timer storage and FIFO queues unless the
  measurements identify them as material costs.
- [ ] If replacement is justified, use a cancellation-safe timed-wait index
  (for example, a heap plus registration IDs), and preserve FIFO channel
  arbitration and winner-removes-losers semantics.

Completion requires either benchmark evidence that the current implementation
is adequate for the supported workload or focused regression tests proving
the selected replacement preserves scheduling semantics.

## Stage 6: compact executable representation

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

## Stage 7: stack-versus-register decision gate

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
