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
- any metric on an execution path is opt-in behind a Cargo feature disabled by
  default, and compiles to a no-op in ordinary builds; and
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
| 5 | Provisional scheduler scaling decision | Timer/select workload measurements | Initial bounded evidence, followed by the scaling audit below |
| 6 | Future compact encoding direction | Program-layout and dispatch measurements | Field-width direction recorded; installation remains future work |
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
- [x] Put every execution-path counter behind a default-disabled `metrics`
  feature before treating benchmark results as ordinary VM performance.
- [x] Keep correctness tests separate from performance thresholds; benchmarks
  inform architecture and do not make timing a flaky test requirement.

The benchmark harness is invoked with `make bench-vm`, which enables the
default-disabled `metrics` feature. It compiles each representative source
workload once, then reports elapsed time plus the per-run VM counters
accumulated across the benchmark: executed instructions, whole-instruction
clones, frame creation, and frame-local binding-cell allocation. Keep
additional measurements, such as `Value` clone or reference-count traffic,
scoped to the representation change that needs them; do not instrument every
dynamic-value clone until a baseline identifies it as a candidate cost.
Program-layout measurements are computed only when explicitly requested and do
not require this feature; runtime counters must not add work to normal
dispatch.

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

### Historical Stage 1 execution plan

Stage 1 was completed through the following narrow slices. The sequence is
retained as implementation history; it is not the current work queue.

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

The slices used unchanged source locations and language frames for failed
calls, unchanged overload selection and argument binding, and reduced
source-span clones as their acceptance criteria. Bytecode verification and
metadata pooling began only after this borrowed-dispatch audit completed.

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

- [x] Add a verifier invoked at the public VM entry boundary before a frame is
  created.
- [x] Validate chunk and constant references, jump targets, local-slot bounds,
  declared arity/local consistency, and metadata-pool indices that can be
  checked without executing values.
- [x] Track conservative operand-stack state through control flow, including
  `TryMatch` success/failure temporaries, and reject statically provable
  underflow without rejecting valid variable-shape joins.
- [x] Keep the dispatch-time checks for all dynamic and verifier-independent
  failures: manually constructed private bytecode remains untrusted.
- [x] Add table-driven malformed-program tests plus generated malformed-program
  coverage
  asserting that arbitrary private bytecode returns `RuntimeError` rather than
  panicking.

Completion requires malformed bytecode to fail deterministically before
observable partial execution whenever the defect is statically knowable, and
all existing source-located runtime diagnostics to remain intact.

### Initial verifier slice

The verifier now runs before the root frame is allocated. It rejects invalid
local slots, jump targets, function-chunk references from constants and closure
creation, selected-call identities, and module-tag metadata references. The VM
retains dispatch-time checks for dynamic values, stack shape, captured slots,
and all operations a manually constructed program can still invalidate at run
time. Focused malformed-bytecode tests prove that the initial structural cases
return `RuntimeErrorKind::InvalidBytecode` before execution begins.

The structural pass also validates `Constant` pool reads and rejects empty
`select` instructions before stack values are consumed. The conservative
control-flow analysis is now enabled for every chunk. It tracks abstract stack
values through jumps, conditionals, calls, recurrence, cleanup, and scheduler
operations, retaining explicit markers for `TryMatch` bindings and result
flags. This models both match paths with their common temporary shape while
joining variable-height paths conservatively, so only underflows possible on a
reachable path are rejected.

Called chunks begin with their retained callee and arguments beneath their
frame-local operand values, while the public entry chunk begins empty. The
verifier seeds those states accordingly; this preserves valid guard fallthrough
and allows the same analysis to cover source-compiled match functions and
manually constructed bytecode.

Match metadata now receives its own structural pass: pinned operands, computed
map keys, nested constrained patterns, and schema-constrained match types must
all reference an operand supplied by the enclosing `TryMatch`. This preserves
the existing missing-operand diagnostic while moving its detection before
execution. The verifier also derives each pattern's binding count—including
alternative consistency and list/map rest bindings—and rejects a mismatched
`TryMatch` declaration before it can perturb the operand stack.

## Stage 3: compact source and opcode metadata

- [x] Intern source paths once per program or source table and identify them
  with `SourceId`.
- [x] Store compact `SpanId` references with bytecode and resolve them to the
  public `SourceSpan` form only when producing a diagnostic.
- [x] Move variable-size opcode data—global names, patterns, capture lists,
  schema fields, and struct field lists—into indexed chunk or program metadata
  pools.
- [x] Evaluate a compressed per-chunk source map after the simple indexed form
  is measured.
- [x] Keep Rust-owned metadata and layout out of the `.cslug` contract.

Completion requires unchanged source-located error behavior, malformed-index
tests for each metadata pool, and before/after instruction-size measurements.
All of these IDs, pools, and Rust layouts remain private implementation detail:
`.cslug` has no encoder or loader yet, and its future versioned representation
must translate into `Program`, `Chunk`, and `Op` rather than serialize them.

### Span-table slice

Programs now own interned source paths and span records. Instructions carry an
optional `SpanId`; `Chunk::emit_at` deduplicates local source spans before
`Program::add_chunk` remaps them into the program table. The VM resolves table
entries for borrowed dispatch and retains an owned public `SourceSpan` only for
errors, frames, throws, and suspension state that outlive the current lookup.
Malformed span indices are rejected before execution, and focused coverage
proves both deduplication and unchanged diagnostic locations.

### Opcode-metadata pool slice

`Program::add_chunk` now lowers build-time global names, captures, schema and
struct fields, and match patterns into checked program-owned pools. Installed
instructions contain typed pool IDs rather than variable-size operands, while
source compilation and focused bytecode construction retain the existing
ergonomic forms. Missing indices fail verification before dispatch; the next
Stage 3 work is to measure the resulting instruction layout and decide whether
the remaining opcode metadata warrants the same treatment. The VM dispatches
against borrowed pool entries directly; it does not reconstruct a legacy
variable-size opcode on the execution path. Closure creation retains a copied
capture recipe only because that recipe outlives the pool lookup.

### Direct pooled-dispatch measurement

The benchmark compared the resolver-based pooled-dispatch baseline with direct
borrowed-pool dispatch. Instruction layout is intentionally unchanged by this
execution-path-only change; instruction bytes remain as follows before and
after the change:

| Workload | Before/after instruction bytes |
|---|---:|
| arithmetic and branches | 2,432 / 2,432 |
| calls and closures | 2,432 / 2,432 |
| pattern matching | 2,816 / 2,816 |
| deferred cleanup | 1,856 / 1,856 |
| lists and maps | 2,624 / 2,624 |

On this local run, direct dispatch was directionally faster in every workload
(about 0–12%, with no claim of portability). The important result is that the
installed compact representation now remains compact through dispatch rather
than being re-expanded per execution.

### Compressed source-map evaluation

The benchmark harness now reports current instruction bytes, inline span-field
bytes, and the estimated bytes for a per-chunk run map. On the representative
programs, the compressed estimate is smaller, but the total instruction-layout
saving is modest:

| Workload | Inline span bytes | Compressed estimate | Saving |
|---|---:|---:|---:|
| arithmetic and branches | 304 | 200 | 34% |
| calls and closures | 304 | 128 | 58% |
| pattern matching | 352 | 144 | 59% |
| deferred cleanup | 232 | 112 | 52% |
| lists and maps | 328 | 168 | 49% |

Because this corresponds to only about 4–8% of whole instruction storage and
would add a source-map lookup on every fetch, the current direct `SpanId`
representation remains in place. Reconsider it with Stage 6's executable
layout decision; see [Defer a compressed per-chunk source map](../decisions/2026-08-30-defer-compressed-source-map.md).

## Stage 4: direct ordinary locals and promoted captures

- [x] Represent an ordinary frame local as a direct `Value`.
- [x] Promote a local to a shared binding cell when a closure captures it.
- [x] Make local reads and writes transparently handle direct and promoted
  storage.
- [x] Emit captures only for outer bindings referenced by a function body,
  using lazy capture creation or explicit free-variable analysis.
- [x] Preserve capture identity when `recur` replaces the active function's
  parameter and local values.

Focused coverage must include uncaptured mutable locals, capture before and
after assignment, sibling closures, captures through intermediate functions,
captured parameters, escaping captures across `recur`, and deferred closures.

Frames now store ordinary locals directly and promote a slot to a shared cell
only when `MakeClosure` captures that slot. Reads and writes handle both slot
forms transparently. The source compiler records captures lazily, including
the required intermediate capture when an inner function reaches past its
parent; unused visible bindings no longer become closure captures. Replacing a
frame's locals during `recur` leaves cells already held by escaping closures
intact, so a closure keeps its iteration's binding identity.

With `metrics` enabled, the current benchmark reports zero local binding-cell
allocations for arithmetic, pattern matching, cleanup, and list/map workloads.
The calls-and-closures workload reports one cell per invocation, corresponding
to its escaping capture. See [Use direct locals and promote escaping captures](../decisions/2026-08-30-direct-locals-promoted-captures.md).

## Stage 5: measure scheduler scaling before replacing queues

- [x] Extend the benchmark corpus with many timers, many `select` cases, and
  cancellation of suspended waits.
- [x] Record timer registration, next-deadline lookup, wakeup, and loser-removal
  costs separately from ordinary VM dispatch.
- [x] Retain the current vector-backed timer storage and FIFO queues unless the
  measurements identify them as material costs.
- [x] Do not replace the queues without evidence; if a replacement becomes justified, use a cancellation-safe timed-wait index
  (for example, a heap plus registration IDs), and preserve FIFO channel
  arbitration and winner-removes-losers semantics.

Completion requires either benchmark evidence that the current implementation
is adequate for the supported workload or focused regression tests proving
the selected replacement preserves scheduling semantics.

The benchmark now includes 32 concurrent timed tasks, one 16-case timed
`select`, and a fail-fast nursery workload with 16 suspended multi-wait tasks.
With `metrics`, it records timer registrations, deadline scans, timer wakeups,
and wait-registration removals. Per invocation, the timed-task workload records
32 registrations, one deadline scan, 32 wakeups, and 96 removals; the 16-case
select records 16, one, one, and 32 respectively. The cancellation-shaped
workload records 32 registrations and 64 removals while preserving the existing
focused cancellation regressions.

On the baseline machine these bounded workloads completed without evidence
that vector scans or FIFO arbitration are material relative to the scheduler
work itself. Keep the current representation and reconsider only if a future,
larger supported workload changes that result. See [Retain measured scheduler queues](../decisions/2026-08-30-retain-measured-scheduler-queues.md).

This is a provisional bounded-workload decision, not a completed scaling
result. The counters record registrations, lookup calls, wakeups, and requested
removals; they do not count vector entries examined by deadline or removal
scans. The current workloads also perform one next-deadline lookup per
invocation. During the 2026-08-30 plan review, the cancellation-shaped workload
reported 16 timer wakeups per invocation and ran for approximately one 50 ms
timer interval, so it did not isolate pre-deadline fail-fast cancellation cost.
The scheduler measurement audit below must close these evidence gaps before a
larger supported workload is declared adequately measured.

## Stage 6: select a future compact executable direction

- [x] Select field widths and instruction formats from measured program limits;
  do not make Rust enum layout or host pointer width part of the format.
- [x] Keep regular operations for loads, moves, arithmetic, comparison, jumps,
  and returns.
- [x] Keep calls, closure creation, collection construction, matching, cleanup,
  throwing, and recurrence as medium-grained semantic operations where that
  avoids repeated dispatch or duplicated validation.
- [x] Do not add a fused superinstruction unless a benchmark identifies a stable
  hot sequence and the added verifier/compiler complexity is justified.

Compact encoding is independent of whether expression temporaries use an
operand stack or frame registers.

The layout report now records maximum chunk length, constant-pool size, local
frame size, and metadata-pool size alongside the 64-byte Rust `Instruction`
layout. The representative corpus reaches at most 525 instructions in one
chunk, 16 constants, one local frame slot, and 33 metadata entries. These are
comfortably below 16-bit limits, but a future installed encoding adopts an
8-bit opcode tag plus 32-bit operand/index fields: `u32` aligns with the
existing checked metadata IDs and leaves room for larger real programs without
making host pointer width part of the design.

No second byte-stream encoding is installed yet. The current typed, verified
private representation remains the executable form until the Stage 7
stack/register experiment can compare total executable size, dispatch, and
compiler complexity against it. Regular stack operations and the existing
medium-grained semantic boundaries remain the selected instruction format; no
benchmark has identified a stable sequence worth fusing. See [Select a future compact executable shape](../decisions/2026-08-30-select-future-compact-executable-shape.md).

Accordingly, this stage records a field-width and instruction-shape direction;
it does not claim that a compact executable representation has been installed.
The current layout report counts inline Rust `Instruction` storage and pool
limits, not heap storage owned by inline vectors and strings, constant and
metadata pool contents, or copies of a program retained by task execution.

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

## Review findings and future work

The 2026-08-30 holistic review found no failing correctness suite, but it
identified ownership, verification, measurement, data-locality, and plan-status
work that must precede a stack/register decision. Work through these items in
order and land each independently measurable change. The existing direct-local
and borrowed-dispatch behavior remains the baseline.

Every completed future-work item below must add a dated **Measurement record**
subsection immediately before the next numbered item. It must identify the
exact command, revision or pre-change derivation, workload sizes, relevant
before/after counters, and byte or allocation accounting where applicable.
It must distinguish measured values from values mechanically derived from the
previous implementation, state what storage the byte estimate excludes, and
avoid treating machine-dependent elapsed time as a threshold. A change is not
complete until its record explains whether semantic checks, diagnostics, and
the non-target scheduler or execution counters remained stable.

### 1. Share installed programs across executions

- [x] Replace per-task and per-nursery `Program::clone` operations with clones
  of one installed `Rc<Program>`.
- [x] Make the public root execution boundary establish that owner once, while
  preserving the checked direct-bytecode API and module-relative closure
  behavior.
- [x] Record full-program clone count and estimated cloned bytes. The legacy
  `&Program` entry points make one root installation copy; the installed
  `Rc<Program>` entry points make no full-program copies on root, task, or
  nursery paths.
- [ ] Re-run scheduler measurements after removing program-copy cost from task
  creation.

Completion requires program storage to remain approximately constant as task
count grows, apart from task-specific frames, stacks, captures, and suspension
state. Add focused coverage for root closures, imported-module closures,
nested nurseries, and tasks spawned by tasks.

#### Measurement record: ownership (2026-08-30)

`make bench-vm` after this change reports whole-program clone counters and
estimated copied inline instruction bytes. The compatibility benchmark uses
the retained `&Program` entry point, so every invocation still makes one root
installation copy. The before values are exact counts derived from the former
per-`Spawn` and per-`Nursery` `Program::clone` sites; that version did not yet
expose a program-clone counter. Estimated bytes use the benchmark's reported
inline instruction storage only, not heap-owned constants or metadata.

| Workload | Runs | Before clones / estimated bytes | After clones / estimated bytes | Change |
|---|---:|---:|---:|---:|
| many timers (32 spawned tasks) | 100 | 3,200 / 141,516,800 | 100 / 4,422,400 | 32× fewer |
| cancelled suspended waits (17 tasks plus nursery body) | 10 | 180 / 2,269,440 | 10 / 126,080 | 18× fewer |

The same run preserved the scheduler-work counters: many timers registered and
woke 3,200 timers, while cancelled suspended waits registered 320 timers and
woke 160 before cancellation. Elapsed time remains machine-dependent and is
therefore not a plan threshold. Future scheduler measurements should use the
installed `Rc<Program>` entry points when they need to exclude the remaining
compatibility installation copy as well.

### 2. Close statically knowable verifier gaps

- [x] Reject every reachable fallthrough past the end of a chunk before a
  frame is created; terminal paths must end in `Return`, `Throw`,
  `MatchFailure`, `NotImplemented`, or another explicitly modeled terminal.
- [x] Reject overflowing operand counts, including `Call(usize::MAX)`, in the
  structural pass instead of representing them as a zero-pop operation.
- [x] Track scope depth through control flow and reject a reachable unmatched
  `LeaveScope` before execution.
- [x] Expand malformed-program generation across opcode families, metadata
  indices, control-flow joins, captures, scopes, calls, and scheduler
  operations.

Tests for pre-execution rejection must also assert that globals, evaluated
module-tag data, task queues, and other observable VM state remain unchanged.
Dispatch-time checks remain required for defects that depend on dynamic
values or closure state.

#### Measurement record: verifier gaps (2026-08-30)

`cargo test --test vm` is the deterministic verification command. Before this
change, a reachable non-terminal final instruction passed structural
validation, `Call(usize::MAX)` reached dispatch through a zero-pop stack
effect, and `LeaveScope` failed only after frame creation. After the change,
focused malformed programs reject those cases, scope-depth conflicts at
control-flow joins, invalid metadata, function references, calls, maps, and
empty scheduler selections before dispatch. The generator combines malformed
opcode families over 64 seeded cases and asserts checked errors without host
panics. The global-mutation regression confirms a rejected program cannot
define a global; no allocation or byte estimate applies because this is a
validation-only change.

### 3. Remove the obsolete owned dispatch body

- [x] Delete the unreachable second opcode interpreter that follows the
  borrowed-dispatch outcome in `execute_raw`.
- [x] Remove owned-span helper variants that become unused, retaining explicit
  ownership only for runtime errors, call frames, suspended state, and other
  durable values.
- [x] Remove the `unreachable_code` lint allowance from the dispatch loop.

Completion requires one authoritative opcode dispatch implementation and the
same VM, CLI, malformed-bytecode, source-location, and call-frame behavior.

#### Measurement record: borrowed dispatch consolidation (2026-08-30)

`cargo test --features metrics --test vm` and `make check` are the
verification commands. Before this change, the executable contained two opcode
dispatch bodies and duplicate owned-span helper families, although the second
body was unreachable after the borrowed dispatcher returned. After the change,
one dispatch body and the borrowed-span helper family remain; only the owned
span conversion required for durable errors, frames, and suspended state is
retained. The metrics-enabled tests preserve zero successful-path source-span
clones in the representative arithmetic dispatch test, while source-location,
call-frame, malformed-bytecode, VM, and CLI coverage remain green. This is a
dead-code removal, so it has no independent allocation or byte estimate.

### 4. Audit scheduler scaling with cost-sensitive evidence

- [ ] Count timer entries examined by deadline lookup and wakeup scans, plus
  channel, task, and timer entries examined during wait removal.
- [ ] Record peak timer, ready-queue, channel-waiter, and task-waiter depth.
- [ ] Exercise several workload sizes rather than one bounded point and report
  work growth alongside elapsed time.
- [ ] Make the cancellation workload prove cancellation before its deadline;
  it must report zero timer wakeups and no timer-duration wall-clock floor.
- [ ] Separate scheduler work from sleeping, VM construction, verification,
  and program ownership costs.

After this audit, either reaffirm the vector-backed queues with stated
supported limits or reopen the cancellation-safe timed-wait-index decision.
FIFO channel arbitration and winner-removes-losers behavior remain mandatory.

### 5. Complete executable and allocation accounting

- [ ] Measure inline instruction storage, heap-owned opcode operands,
  constants, metadata pools, source tables, capacities, and shared versus
  duplicated program storage.
- [ ] Decide whether the remaining inline interpolation, spread, call,
  import, `select`, and `recur` descriptors should use typed pools.
- [ ] Measure verifier time separately and decide whether an immutable
  `VerifiedProgram` installation boundary should avoid repeated whole-program
  verification.
- [ ] Add scoped `Value` clone, move, drop, and reference-count traffic for the
  Stage 7 prototype rather than treating instruction bytes as total executable
  size.

Do not describe the current 64-byte typed instruction as a compact executable
encoding. Installing an 8-bit-tag/32-bit-operand representation remains a
future change that requires before/after total-size and dispatch evidence.

### 6. Make optimization metrics a tested contract

- [ ] Run metrics-enabled VM tests in the normal handoff and CI gate while
  keeping the feature disabled in ordinary runtime builds.
- [ ] Count source-span ownership consistently on diagnostic error paths, not
  only successful durable-frame and suspension paths.
- [ ] Add focused failing-call, thrown-error, suspended-select, task, and
  native-error metric tests.
- [ ] Retire counters that no longer provide actionable regression evidence,
  or make their instrumentation capable of detecting the regression they
  claim to measure.

Timing remains benchmark evidence rather than a correctness assertion. Stable
representation counters may use exact assertions when their ownership
boundary is intentional and documented.

### 7. Improve hot-path data locality before changing the operand model

The current implementation already keeps each chunk's instructions, the
operand stack, frame list, ordinary local slots, and scheduler queues in
contiguous Rust collections. Direct locals also avoid a binding-cell allocation
and pointer chase until capture promotion is required. The remaining hot path
still combines a 64-byte instruction, eager span-table access, separately
allocated frame-local vectors, string-keyed global lookup, and pointer-heavy
runtime objects. Improve those costs in measured, independent slices before
attributing them to the stack operand model.

- [ ] Extend layout reporting with `Value`, `LocalSlot`, `Frame`, closure,
  task-state, and instruction size/alignment, plus allocation count and peak
  resident-memory measurements for representative workloads.
- [ ] Keep `SpanId` on the ordinary dispatch path and resolve it to
  `SourceSpan` only when an error, call frame, suspension, or another durable
  owner needs source information.
- [ ] Prototype one VM-owned contiguous local-slot arena. Frames retain a
  local-range base and length; calls append slots, returns truncate them, and
  `recur` replaces the active range without changing captured-cell identity.
- [ ] Compare compiler-resolved global-slot IDs backed by contiguous module
  storage with the current pooled-name plus string-`HashMap` lookup. Retain the
  name map for linking, imports, host access, conflicts, and diagnostics.
- [ ] After full executable accounting, prototype a fixed-width typed
  instruction with an opcode tag and 32-bit operands or metadata IDs. Measure
  a practical 16--24-byte target against the current 64-byte Rust enum; do not
  assume that a byte stream is required.
- [ ] Profile instructions per cycle, branch misses, L1 data misses, and
  last-level cache misses where supported. Keep portable allocation, size, and
  representation counters as the cross-platform baseline.
- [ ] Measure collection access before replacing the cache-friendly small
  vector representation of maps or other values with a pointer-heavier index.

Each prototype must report dispatch time, allocations, peak memory, cache or
portable locality proxies, `Value` clone/drop traffic, and changes in verifier
or compiler complexity. It must retain checked failures, source locations,
call frames, mutable-capture identity, cleanup behavior, scheduler semantics,
and `recur` behavior.

Do not introduce manual prefetching, a custom allocator, unsafe tagged
pointers, forced cache-line alignment, task arenas, or scheduler handle tables
without a profile showing that shared program ownership, lazy span resolution,
contiguous locals, indexed globals, and denser instructions are insufficient.
Rust allocator addresses are not themselves an optimization contract; favor
compact contiguous ownership and fewer allocations over attempts to prescribe
absolute memory locations.

### 8. Strengthen the Stage 7 comparison protocol

- [ ] Add warmup and repeated samples and report distributions rather than one
  aggregate `Instant` measurement.
- [ ] Separate compiler, verifier, VM setup, dispatch, scheduler wait, and
  teardown time.
- [ ] Include representative real modules and size-scaled synthetic workloads
  in addition to the small feature-focused corpus.
- [ ] Compare peak memory and allocation traffic as well as instruction count,
  elapsed time, and executable size.

Only begin a production register lowering after the shared-program,
verification, scheduler-measurement, full-layout, and hot-data-locality audits
above are closed. A prototype may proceed earlier only as an isolated
experiment whose results do not determine the production operand model.
