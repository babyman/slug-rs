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

`make measure-vm-memory` builds the release CLI and runs each checked-in source
fixture in its own process, reporting peak resident memory in bytes. It uses
`/usr/bin/time -l` on macOS and GNU `time` on Linux. The fixture set establishes
a minimal source-execution baseline plus retained-closure workloads at 128 and
1,024 items. Peak RSS includes process startup, the allocator, and mapped
runtime code, so it is directional host evidence rather than a portable CI
threshold.

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
- [x] Retain FIFO ready queues and replace the timer vector only after the
  measurements identify its scans as material costs.
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

The initial bounded workloads retained the vector representation, but the later
128-timer scaling result made its scan work material. Timers now use an indexed
heap while FIFO arbitration remains unchanged. See [Use an indexed timer
heap](../decisions/2026-09-14-indexed-timer-heap.md).

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
frame size, and metadata-pool size. The representative corpus reaches at most 525 instructions in one
chunk, 16 constants, one local frame slot, and 33 metadata entries. These are
comfortably below 16-bit limits, but a future installed encoding adopts an
8-bit opcode tag plus 32-bit operand/index fields: `u32` aligns with the
existing checked metadata IDs and leaves room for larger real programs without
making host pointer width part of the design.

Installed programs now lower builder instructions into a private 24-byte packed
representation with three `u32` operands and pooled variable metadata. Regular
stack operations and the existing medium-grained semantic boundaries remain
the selected format; no benchmark has identified a stable sequence worth
fusing. See [Pack installed bytecode](../decisions/2026-09-14-packed-installed-bytecode.md).

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
  of one VM-owned installed program.
- [x] Make the public root execution boundary establish that owner once, while
  preserving the checked direct-bytecode API and module-relative closure
  behavior.
- [x] Record full-program clone count and estimated cloned bytes. The legacy
  `&Program` entry points make one root installation copy; VM-created
  `InstalledProgram` entry points make no full-program copies on root, task,
  or nursery paths.
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
therefore not a plan threshold. Future scheduler measurements should use
VM-created `InstalledProgram` values when they need to exclude the remaining
compatibility installation copy as well.

#### Measurement record: VM-owned installation (2026-09-14)

Command: `cargo bench -p slug-vm --bench vm --features metrics`. The benchmark
now installs each compiled program once, then executes its
`InstalledProgram` repeatedly. In the prior compatibility-wrapper benchmark,
the 1,000-run arithmetic workload reported 1,000 whole-program clones and
1,000 validation passes. The installed benchmark reports zero whole-program
clones and one installation validation for the same workload. The 1,000-run
ordinary-call workload has the same 0/1 clone/validation result. Task, frame,
and scheduler counters are unchanged by this ownership move; elapsed times are
machine-local evidence only and are not compared as a threshold.

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

- [x] Count timer entries examined by deadline lookup and wakeup scans, plus
  channel, task, and timer entries examined during wait removal.
- [x] Record peak timer, ready-queue, channel-waiter, and task-waiter depth.
- [x] Exercise several workload sizes rather than one bounded point and report
  work growth alongside elapsed time.
- [x] Make the cancellation workload prove cancellation before its deadline;
  it must report zero timer wakeups and no timer-duration wall-clock floor.
- [x] Separate scheduler work from sleeping, VM construction, verification,
  and program ownership costs.

After this audit, either reaffirm the vector-backed queues with stated
supported limits or adopt the cancellation-safe timed-wait index.
FIFO channel arbitration and winner-removes-losers behavior remain mandatory.

#### Measurement record: scheduler scaling (2026-08-30)

`make bench-vm` runs the scaled timer workloads with the default-disabled
`metrics` feature. Before this audit, the scheduler exposed only registrations,
lookups, wakeups, and registration removals; entries examined, peaks, and
blocked scheduler time were not measured. After it, the benchmark distinguishes
time blocked in `SchedulerSignal::wait` from total wall time, while compilation
is outside the measured loop and program-clone counters report the remaining
root installation work.

| Timer tasks per run | Runs | Deadline entries examined | Wakeup entries examined | Peak timer / ready depth |
|--------------------:|-----:|--------------------------:|------------------------:|-------------------------:|
|                   8 |  100 |                       800 |                   2,400 |                    8 / 8 |
|                  32 |  100 |                     3,200 |                   9,616 |                  32 / 32 |
|                 128 |   25 |                     3,200 |                   9,729 |                128 / 128 |

Entry examination and peak depth grow with queue size as expected for the
retained vector-backed timer queue. The 8-, 32-, and 128-task runs spent about
125 ms, 121 ms, and 25 ms respectively blocked in scheduler waits; the latter
two use different repetition counts, so wall-clock values are directional only.
Removal counters also now report channel, task, and timer entries examined and
their observed queue peaks. The cancellation workload puts the failing task
first: across 10 runs it registered and woke zero timers, spent 0 ns in the
scheduler wait, and completed in about 1.1 ms total—there is no 50 ms timer
floor. The FIFO queues remain retained. The later 128-timer result reopened the
timed-wait-index decision and led to the indexed heap described below.

#### Measurement record: indexed timer heap (2026-09-14)

Command: `cargo bench -p slug-vm --bench vm --features metrics`. The indexed
heap preserves the 8/32/128-timer workloads' registration and wakeup counts,
but reduces deadline-root accesses and due-entry work to one per pending or due
timer. At 128 timers across 25 runs, deadline entries fell from 3,200 to 25 and
wakeup entries from 219,858 to 3,200. Timer-registration lookups during loser
removal were 3,200. Scheduler wait time remains machine-dependent and is not a
threshold; source behavior and the focused cancellation tests are unchanged.

### 5. Complete executable and allocation accounting

- [x] Measure inline instruction storage, heap-owned opcode operands,
  constants, metadata pools, source tables, capacities, and shared versus
  duplicated program storage.
- [x] Decide whether the remaining inline interpolation, spread, call,
  import, `select`, and `recur` descriptors should use typed pools.
- [x] Measure verifier time separately and decide whether an immutable
  `VerifiedProgram` installation boundary should avoid repeated whole-program
  verification.
- [x] Reserve scoped `Value` clone, move, drop, and reference-count traffic for
  the Stage 7 prototype rather than treating instruction bytes as total
  executable size. It is deliberately not installed as broad production
  instrumentation before that prototype exists.

Do not describe the current 64-byte typed instruction as a compact executable
encoding. Installing an 8-bit-tag/32-bit-operand representation remains a
future change that requires before/after total-size and dispatch evidence.

#### Measurement record: executable and allocation accounting (2026-08-30)

Command: `make bench-vm` (which enables the opt-in `metrics` feature).
`Program::layout_metrics` now reports the inline `Program` object, vector
capacities for chunks and constants, remaining inline opcode descriptors,
indexed metadata pools, and source tables. It intentionally does not claim
allocator-resident bytes for `HashMap` buckets, nested `Value` heap objects, or
the contents owned by nested metadata values. The shared-versus-duplicated
program counter remains `program_clones` and its estimated copied instruction
bytes, added in item 1.

| Workload                        | Program inline | Chunk storage | Constant capacity | Descriptor capacity | Metadata capacity | Source-table capacity | Verification time |
|---------------------------------|---------------:|--------------:|------------------:|--------------------:|------------------:|----------------------:|------------------:|
| calls and closures (1,000 runs) |          512 B |       3,068 B |             160 B |                48 B |             480 B |                 448 B |            2.8 ms |
| 32 timed tasks (100 runs)       |          512 B |      83,152 B |             320 B |                66 B |           5,760 B |               6,208 B |           17.3 ms |
| 128 timed tasks (25 runs)       |          512 B |     330,064 B |             320 B |               258 B |          22,656 B |              24,640 B |            7.0 ms |

The capacity figures are for one compiled program; verification time is the
aggregate across the stated runs and is directional machine-local evidence.
The remaining descriptor vectors are small in this corpus, so they stay inline
to preserve straightforward source construction. Pool them only if a
descriptor-specific allocation or dispatch measurement identifies a material
cost. Repeated validation is now measured separately and `Vm::install` owns
the immutable `InstalledProgram` boundary; borrowed `&Program` entry points
remain checked compatibility wrappers. `Value` traffic accounting is deferred
to the Stage 7 prototype, where its scope can distinguish register-lowering
traffic from the existing stack VM rather than imposing broad, non-actionable
instrumentation.

### 6. Make optimization metrics a tested contract

- [x] Run metrics-enabled VM tests in the normal handoff and CI gate while
  keeping the feature disabled in ordinary runtime builds.
- [x] Count source-span ownership consistently on diagnostic error paths, not
  only successful durable-frame and suspension paths.
- [x] Add focused failing-call, thrown-error, suspended-select, task, and
  native-error metric tests.
- [x] Retire counters that no longer provide actionable regression evidence,
  or make their instrumentation capable of detecting the regression they
  claim to measure.

Timing remains benchmark evidence rather than a correctness assertion. Stable
representation counters may use exact assertions when their ownership
boundary is intentional and documented.

#### Measurement record: optimization metrics contract (2026-08-30)

Command: `make test` (and therefore `make check`). Before this slice, the
normal test target omitted `--features metrics`, so feature-gated counter tests
did not run in the ordinary handoff gate. After it, `make test`, `make test-vm`,
and `make test-cli` all enable `metrics`, while normal library and binary builds
remain feature-disabled by default.

| Contract path | Before | After |
| --- | --- | --- |
| Normal handoff gate | Metrics tests excluded | Library, binary, and integration tests compile and run with `metrics` |
| Borrowed-span diagnostic | Error construction was not counted | `error_at` acquires its owned span through the counter |
| Failing call, throw, native error | No focused ownership assertions | Each source-located path asserts one owned-span boundary |
| Suspended `select`, spawned task | Scheduler counters only | Each durable state path asserts one owned-span boundary |

The existing exact instruction-clone, program-clone, and capture-promotion
tests remain actionable, and scheduler queue counters retain dedicated
assertions. No counter was retired: every retained counter now has a concrete
representation or ownership regression it can expose. `verification_time` and
`scheduler_wait_time` remain intentionally unasserted timing evidence; their
values belong in repeated benchmarks rather than correctness tests.

### 7. Improve hot-path data locality before changing the operand model

The current implementation already keeps each chunk's instructions, the
operand stack, frame list, ordinary local slots, and scheduler queues in
contiguous Rust collections. Direct locals also avoid a binding-cell allocation
and pointer chase until capture promotion is required. The remaining hot path
still combines a 64-byte instruction, eager span-table access, separately
allocated frame-local vectors, string-keyed global lookup, and pointer-heavy
runtime objects. Improve those costs in measured, independent slices before
attributing them to the stack operand model.

- [x] Extend layout reporting with `Value`, `LocalSlot`, `Frame`, closure,
  task-state, task-execution, and instruction size/alignment. Static layout
  reporting excludes heap allocations and allocator bookkeeping.
- [ ] Add portable allocation-count and peak-resident-memory measurements for
  representative workloads; use an external profiler or a narrowly scoped
  allocator experiment rather than a misleading in-process estimate.
- [x] Keep `SpanId` on the ordinary dispatch path and resolve it to
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

#### Measurement record: locality reporting and lazy spans (2026-08-30)

Command: `make bench-vm` (with `metrics`). Before this slice, dispatch resolved
the program span table once for every fetched instruction. After it, dispatch
retains the `SpanId` in VM state and resolves the table only when a diagnostic,
call frame, spawned task, or suspension state takes ownership. The benchmark
reports both owned-span clones and the corresponding source-table lookups.

| Workload                             | Instructions | Before span-table lookups | After source-table lookups |
|--------------------------------------|-------------:|--------------------------:|---------------------------:|
| arithmetic and branches (1,000 runs) |    2,825,000 |                 2,825,000 |                      1,000 |
| calls and closures (1,000 runs)      |       32,000 |                    32,000 |                      2,000 |
| lists and maps (1,000 runs)          |       35,000 |                    35,000 |                          0 |

The same run reports host-local deterministic layouts: `Value` 40 B/8-byte
alignment, `LocalSlot` 40 B/8, `Frame` 152 B/8, `Closure` 72 B/8, `Task` 8
B/8, `TaskState` 192 B/8, `TaskExecution` 464 B/8, and `Instruction` 64 B/8.
These figures exclude vector capacities, heap payloads, reference-counted
objects, and allocator metadata. The arena, global-slot, and fixed-width
instruction proposals remain unchecked because no allocation, locality, or
dispatch evidence yet justifies changing their current representations. Cache
counter collection and collection-index changes likewise remain external
profiling gates.

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

### 9. Optimize the remaining source-level call and tree bottlenecks

The 2026-09-20 release-profile measurement makes ordinary calls and
binary-tree construction/matching the next source-level targets. This is a
private-runtime sequence governed by
[Measure call and tree VM costs before changing representations](../decisions/2026-09-20-measure-call-and-tree-vm-costs.md).
It does not change Slug syntax, semantics, diagnostics, or the portable
`.cslug` contract.

- [x] Add in-process `function-call` and `binary-trees` workloads equivalent
  to the checked-in source benchmarks. Retain their output assertions and
  compile each program once before repeated VM execution.
- [x] Record packed direct-dispatch and rich-op fallback instruction counts.
  Keep the counters behind `metrics`, assert their intended ownership in VM
  tests, and remove them if they stop distinguishing a decision.
- [x] Use the two workloads to establish frames, frame-local vector capacity,
  exact/generic call binding, collection construction, collection elements,
  and collection lookup baselines.
- [ ] If those counters cannot distinguish the binary-tree cost, add a
  temporary scoped experiment for `Value` cloning and `Rc` retain/release
  traffic. Do not retain broad production accounting without an actionable
  representation decision.
- [x] Prototype frame-transition-only synchronization of the VM's current
  global environment. Update it on frame push, return, recovery, and other
  frame-stack changes rather than at every instruction. Preserve frame-owned
  globals for imported and retained interactive closures, cleanup actions,
  suspension, and diagnostics.
- [x] Re-run the source-aligned call benchmark. If frame-local allocation is
  still material, compare a VM-local local-vector recycler with the planned
  contiguous local-slot arena; retain capture identity and `recur` behavior
  in either experiment.
- [x] Use the binary-tree result to decide whether a two-element-list
  construction fast path or additional packed match dispatch is justified.
  Do not add either merely because it is common in this one synthetic shape.
- [x] Re-run `make bench-source`, `make bench-vm`, and `make check` after each
  retained slice. Append a dated measurement record here with command,
  revision, profile, workload sizes, counter deltas, and diagnostic/capture
  regression coverage.

#### Measurement record: source benchmark and task selection (2026-09-20)

Command: `make bench-source`, on revision
`465d7d39bcdb2c180bbc1222f0b054cce5cc4717`, with a clean worktree, three
warmups, and 15 timed samples per runtime/workload. The release profile uses
`opt-level = 3`, LTO, one codegen unit, stripped symbols, and aborting panics.
The earlier packed-dispatch figures used the size-optimized release profile,
so this is a profile comparison rather than isolated VM-change attribution.

| Workload | Slug median | CPython median | Slug / CPython |
|---|---:|---:|---:|
| function-call | 44.678 ms | 25.953 ms | 1.72x |
| n-body | 18.913 ms | 21.500 ms | 0.88x |
| typed-n-body | 18.490 ms | 21.370 ms | 0.87x |
| spectral-norm | 7.680 ms | 19.986 ms | 0.38x |
| typed-spectral-norm | 7.110 ms | 20.031 ms | 0.35x |
| binary-trees | 49.839 ms | 29.049 ms | 1.72x |

`make bench-vm` on the same revision reports `ordinary-calls-200` completing
1,000 runs in 101.709 ms while creating 202,000 frames and 202,000
frame-local vectors. It executes 3,431,000 instructions, performs 201,000
exact positional closure calls, and uses no generic call binding. Existing
benchmarks do not yet make the equivalent binary-tree call, list, and match
cost visible; that is the first checklist item above. These timing figures are
host-local evidence, not performance assertions.

#### Measurement record: source-aligned in-process workloads (2026-09-20)

Command: `cargo bench -p slug-vm --bench vm --features metrics`, after adding
the two workloads at the checked-in source sizes and compiling each one once
before its repeated executions. The benchmark asserts the direct evaluation
results on every run: `100000` for `function-call` and `65535` for
`binary-trees`.

| Workload | Runs | Instructions | Frames | Local vectors / capacity | Exact calls | Collections / elements |
|---|---:|---:|---:|---:|---:|---:|
| function-call | 10 | 17,000,310 | 1,000,020 | 1,000,020 / 1,000,020 | 1,000,010 | 0 / 0 |
| binary-trees | 10 | 19,988,280 | 1,310,710 | 1,310,710 / 2,621,400 | 1,310,700 | 327,670 / 655,340 |

The benchmark still uses the existing installed-program boundary, so both
workloads performed one installation validation and no whole-program clones.
No runtime representation changed in this slice; source diagnostics, capture
behavior, and scheduler counters remain outside these synchronous workloads.

#### Measurement record: packed-dispatch accounting (2026-09-20)

Command: `cargo bench -p slug-vm --bench vm --features metrics`, after adding
the feature-gated counters. Focused VM coverage asserts that a four-instruction
packed-only program records four direct dispatches and no fallback, while a
program with one pooled `DefineGlobal` records three direct dispatches and one
rich-op fallback.

| Workload | Instructions | Packed direct | Rich-op fallback |
|---|---:|---:|---:|
| function-call (10 runs) | 17,000,310 | 16,000,230 | 1,000,080 |
| binary-trees (10 runs) | 19,988,280 | 17,694,500 | 2,293,780 |

The two categories sum to the executed-instruction count in both workloads.
They are opt-in metrics only: ordinary builds retain no counter updates.

#### Measurement record: call and collection baseline (2026-09-20)

Command: `cargo bench -p slug-vm --bench vm --features metrics`. The existing
frame, binding, and collection counters distinguish the two target shapes, so
this slice does not add broad `Value` or reference-count accounting.

| Workload | Frames | Local vectors / capacity | Exact / generic calls | Collections / elements | Lookups |
|---|---:|---:|---:|---:|---:|
| function-call (10 runs) | 1,000,020 | 1,000,020 / 1,000,020 | 1,000,010 / 0 | 0 / 0 | 0 |
| binary-trees (10 runs) | 1,310,710 | 1,310,710 / 2,621,400 | 1,310,700 / 0 | 327,670 / 655,340 | 0 |

All calls in both source-aligned programs use exact positional binding. The
tree workload's collection construction and element counts are substantial,
whereas collection lookup is absent; the next decision therefore needs to
separate construction/match work from the rich-op fallback before adding
value-traffic instrumentation.

#### Measurement record: frame-transition global synchronization (2026-09-20)

The VM now synchronizes its current global environment when a root, call, or
cleanup frame is pushed and when a return or recovery pops a frame. The
per-instruction synchronization assignment was removed. `make bench-source`,
`make bench-vm`, and `make check` passed; elapsed times remain host-local and
were not used as a retention threshold. The VM, module, interactive, cleanup,
and scheduler suites preserve frame-owned imported and retained closures.

#### Measurement record: local-vector recycler comparison (2026-09-21)

The recycler supplies a cleared prior frame-local vector after a frame returns,
without retaining captured cells. Across ten in-process runs it reused 999,990
of 1,000,020 frame entries in `function-call`, and 1,310,540 of 1,310,710 in
`binary-trees`. The same release benchmark recorded 569.442 ms and 591.665 ms
respectively; elapsed time is host-local and did not demonstrate an improvement
over the pre-recycler measurements. The recycler remains an isolated private
experiment; no contiguous local-slot arena is adopted without stronger
allocation and locality evidence. Focused capture and `recur` coverage passed.

#### Measurement record: binary-tree fast-path decision (2026-09-21)

The binary-tree workload performs 1,310,700 exact calls, constructs 327,670
collections with 655,340 supplied elements, and executes 2,293,780 rich-op
fallback instructions across ten runs. Those aggregate counters do not show a
dominant two-element-list construction or match-dispatch cost, so this plan
does not add either specialized fast path. The existing packed dispatch and
collection instrumentation remain the decision evidence.

#### Measurement record: packed scope entry (2026-09-21)

`EnterScope` was the dominant remaining rich-op fallback in the source-aligned
call workload: each non-terminal `recur` iteration enters its `if` branch and
restarts before its matching `LeaveScope` executes. The packed dispatcher now
performs the same checked scope-depth update directly; the rich-op path
retains the shared helper for builder-facing execution. A focused metrics test
proves a packed `EnterScope` no longer unpacks while a following `LeaveScope`
remains a fallback.

On `cargo bench -p slug-vm --bench vm --features metrics`, direct/fallback
dispatch changed from 16,000,230 / 1,000,080 to 17,000,240 / 70 across ten
`function-call` runs, and from 17,694,500 / 2,293,780 to 18,349,850 /
1,638,430 across ten `binary-trees` runs. The in-process elapsed samples were
498.787 ms to 477.552 ms for calls and 548.682 ms to 532.710 ms for trees.
Three independent 15-sample `make bench-source` runs reported function-call
medians of 45.026, 44.489, and 44.641 ms and binary-tree medians of 49.890,
49.130, and 49.406 ms. Source-level timing is within ordinary local variation,
so the retained evidence is the eliminated hot-path fallback and the
in-process result, not a portable end-to-end performance claim.

#### Measurement record: packed match execution (2026-09-21)

`TryMatch` now uses the packed instruction's validated pattern ID directly and
shares the existing binding-count and checked-error logic with the rich opcode.
Across ten binary-tree runs, packed direct dispatch rose from 18,349,850 to
19,332,890 while rich fallback fell from 1,638,430 to 655,390: 983,040 match
instructions no longer unpack a rich opcode. The local-vector recycler was
reapplied between these measurements, so its 623.400 ms combined in-process
tree checkpoint and the accompanying source benchmark are not an isolated
timing comparison for packed matching. The remaining recurring fallback is
`LeaveScope`; measure its no-defer path separately before changing it.

#### Measurement record: packed empty-scope exit (2026-09-21)

The packed dispatcher now exits a scope directly only when no defer scopes
have been materialized and the frame is not a cleanup action. Other exits
return to the rich-op cleanup path unchanged. Across ten binary-tree runs,
direct/fallback dispatch changed from 19,332,890 / 655,390 to 19,988,240 / 40,
eliminating the 65,535 ordinary `LeaveScope` operations per execution. With
the recycler active, the one in-process checkpoint was 556.865 ms for trees
and the 15-sample source median was 50.638 ms. Those are combined-representation
measurements, not an isolated performance claim; repeat them before selecting
another runtime representation change.

#### Measurement record: exact-call stack-to-local entry (2026-09-21)

Exact positional closure entry previously resolved stack arguments into a
transient `Vec<Value>` before immediately copying them into a recycled
frame-local vector. The direct path now resolves each operand-stack argument
into its final local slot, retaining the checked name-resolution failure and
the frame-owned closure/global behavior. The frame-local recycler records
reuse only after a frame actually enters, so a failed resolution cannot be
counted as a successful reuse.

On `cargo bench -p slug-vm --bench vm --features metrics`, the source-aligned
workloads recorded zero exact temporary argument vectors. One ten-run local
checkpoint changed `function-call` from 513.175 ms to 471.000 ms and
`binary-trees` from 553.175 ms to 491.750 ms; their instruction, call, frame,
and collection counts were unchanged. A subsequent 15-sample
`make bench-source` run on a dirty worktree reported 44.438 ms for
`function-call` (1.56x Slug / CPython) and 46.857 ms for `binary-trees`
(1.48x), compared with the immediately preceding baseline of 46.877 ms
(1.72x) and 50.912 ms (1.64x). CPython medians vary locally, so the retained
evidence is the removed per-call allocation plus the paired VM checkpoints;
these source ratios are not portable claims.

### 10. Reduce per-instruction overhead and indexed member lookup (deferred)

Profiling the native-call path on 2026-10-11 found two general costs that are
deferred while typed native calls are implemented.

- [ ] Reduce per-instruction overhead. A plain global read discarded as a
  statement (`GetGlobal`, `Pop`) costs about 44 ns over an empty `recur` loop
  on the measuring host, so roughly 20 ns per dispatched instruction before
  the operation's own work. This dominates short operations such as module
  member reads and should be addressed before adding further
  superinstructions.
- [ ] Make module member lookup independent of export count. `GetIndex` on an
  import map scans its entries in reverse, about 1.7 ns per entry; a
  64-export module adds about 100 ns in the worst position. The largest
  shipped module has 9 exports, so this is not yet material.

A position hint checked only by key is unsound for this: `Value::Map` keeps
duplicate keys (a lookup returns the last one), so a hint naming an earlier
duplicate would return the wrong value. A per-site runtime cache keyed by map
identity is sound only if it holds a reference that also prevents in-place
unique-owner map updates, which would regress update loops. The preferred
direction is a dedicated module-namespace value with indexed lookup, which
needs its own decision record.

#### Measurement record: native-call path and constant-key indexing (2026-10-11)

The completed slices are recorded in
[`completed/typed-native-call-dispatch-spike-results.md`](completed/typed-native-call-dispatch-spike-results.md)
and the dated
[benchmark baseline entry](../engineering/benchmark-baseline-2026-09-16.md#native-call-path-and-owned-value-resolution--2026-10-11).
`GetIndexConstant` (`ddfbe73`) then fused `Constant` and `GetIndex` for literal
keys. On the private profiling harness (median of five 2,000,000-iteration
runs, release build), a member read of a one-export module fell from 228 to
216 ns per loop iteration and a native call through an import from 311 to
301 ns; the 64-export worst case fell from 336 to 319 ns. Semantic checks,
duplicate-key behavior, and missing-key `nil` results are unchanged and
covered by `constant_key_indexing_matches_stack_key_indexing`.

### 11. Append to uniquely owned collections in place (deferred)

Deferred on 2026-10-11 while typed native calls are completed: make it work,
make it right, then make it fast. Typed `slug.db.sqlite` builds query results
in Slug with `collected + [row]` and each row with `values + entry`.
[Its step 2 measurement](completed/typed-native-calls-step-2.md#5-tests-and-measurement)
showed the per-row query cost growing with the result size, and building an
8,000-element list with `acc + [x]` in a `recur` loop took 0.11 s against
0.04 s for 4,000, so each list append copies the whole list.

- [x] Append to a list in place when the left operand is the only reference,
  as unique-owner map updates already do, and measure `+` on lists in a loop.
  Done by moving locals on their last read; see item 12.
- [x] Re-measure typed SQLite queries, and decide whether a per-row C helper
  that reads all of a row's columns in one call is still worth adding. Query
  cost no longer grows with result size; the helper stays undecided until a
  workload needs the remaining per-value native-call cost cut (item 12).

### 12. Value, ownership, and hot-path sweep (proposed)

The 2026-10-12 sweep revisited items 10 and 11 alongside a wider VM review.
Each candidate was judged on execution time, runtime footprint, and
implementation simplicity; the largest wins improve all three. The spikes are
measurement-only commits on local branch `spike/vm-sweep` (workloads and the
interleaved A/B harness are in its `spike/` directory). They are not merge
candidates.

Recommended order:

- [x] **24-byte `Value`.** Store list-view bounds as `u32`, make
  `CallableIdentity` a thin `Rc`, and keep a declared native's identity and
  resource signature behind one `Rc`. `Value` falls from 40 to 24 bytes, which
  shrinks the operand stack, locals, list elements, and map entries. About 25
  changed lines.
- [x] **Move locals on their last read (closes item 11).** An installation
  pass marks each `GetLocal` after which the slot is dead, and the VM moves
  the value out instead of cloning it. A sole-owner list then appends in
  place: a 20,000-element `acc :+ x` loop drops from 0.57 s to under 10 ms.
  Liveness is a single fixed-point pass over the packed code. `Recur`,
  `Return`, and `Throw` end liveness, `SetLocal` kills a slot, and
  `MakeClosure` keeps every slot live. A promoted (captured) slot is still
  cloned. The pass must also run on `.cslug`-loaded chunks.
- [x] **Fast exits on the call path (most of item 10).** Return without going
  through the cleanup queue when no defer scope, cleanup action, or pending
  cleanup exists. Restart an exact positional `recur` by moving stack
  arguments straight into direct locals, with no temporary vector or closure
  clone. Keep the frame's `Rc<Program>` across instructions instead of cloning
  it for every dispatch.
- [x] **Cheap global hashing.** Global names are short identifiers, so an
  in-tree FNV-1a hasher for the global environment replaces SipHash. This
  defers compiler-resolved global slots (item 7) until a profile needs them.
- [x] **Cold error construction.** Mark `error`/`error_at` `#[cold]` and
  `#[inline(never)]`.
- [x] **One packed dispatch loop.** This is the largest simplicity and size
  win, but it must be a deliberate rewrite rather than a deletion. The
  rich-`Op` fallback reconstructs an `Op` from the packed instruction, and 397
  of its lines are unreachable or duplicate the packed hot path: un-pooled
  variants that `pack_instruction` rejects, plus re-implementations of packed
  ops. The `span: Option<&SourceSpan>` parameter on 45 helpers is always
  `None` since diagnostics resolve the active `SpanId` lazily, and the owned
  `call` path duplicates `call_at` for the root call alone. Deleting only
  the dead arms made dispatch about 25–30% slower: LLVM merged the loop
  differently (measured with interleaved runs, so not host drift). The rewrite
  should therefore match once on `PackedOpcode`, keep the instruction pointer
  and chunk in loop locals that are refreshed on frame changes, and put
  explicit `#[inline]`/`#[cold]` boundaries on rare operations. Accept it only
  on an interleaved A/B run.

Measured but not recommended now: returning `Box<RuntimeError>` from
`VmResult` (104-byte to pointer-sized error) showed no execution change
within noise and saved 16 KB, while changing the public error API. A module
namespace value (item 10, second bullet) remains immaterial at nine exports.

#### Measurement record: sweep spikes (2026-10-12)

Command: `python3 -I spike/ladder.py` on branch `spike/vm-sweep`. It runs
seven interleaved rounds of every binary per workload and reports the median
wall-clock seconds. Release profile, base `e9b740c`. Workloads: 5,000,000
`function-call` iterations (fc), 5,000,000 discarded global reads (gg),
binary trees at depth 19 (bt), fannkuch-redux over eight values (fk), and a
20,000-element list append (ap).

| Step (cumulative) | fc | gg | bt | fk | ap | bt peak RSS |
|---|---:|---:|---:|---:|---:|---:|
| Base | 1.49 | 0.82 | 0.52 | 1.10 | 0.67 | 68.7 MB |
| 24-byte `Value` | 1.24 | 0.66 | 0.43 | 0.96 | 0.57 | 52.5 MB |
| + return/recur fast paths | 1.02 | 0.54 | 0.44 | 0.92 | 0.57 | 52.5 MB |
| + cached frame program | 0.95 | 0.50 | 0.41 | 0.87 | 0.58 | 52.5 MB |
| + move on last read | 0.94 | 0.49 | 0.41 | 0.79 | <0.01 | 52.6 MB |
| + FNV globals, cold errors | 0.88 | 0.40 | 0.38 | 0.75 | <0.01 | 52.5 MB |

Running sequential, non-interleaved runs drifted by up to 30% within one
session on the measuring laptop, so future comparisons should use interleaved
A/B runs. With `make bench-source`, `function-call` went from 34.3 to 20.8 ms
(1.34x to 0.80x CPython), `binary-trees` from 35.5 to 27.1 ms (1.25x to
0.93x), and `fannkuch-redux` from 117.7 to 81.7 ms (3.87x to 2.66x). The base
and spike runs were taken hours apart, so these are directional only. The full
workspace suite passed on the combined spike except
`records_direct_local_vector_reuse_across_recur`. The fast `recur` path does
not yet record that metric. Fannkuch's remaining gap comes from `+:` prepend
and from appends to list views, which always copy. That belongs to the
collection-representation work rather than to dispatch.

#### Measurement record: 24-byte `Value` (2026-10-12)

Command: the interleaved A/B harness from `spike/vm-sweep`, seven rounds,
release profile, comparing the pre-sweep binary with this change. `Value` falls
from 40 to 24 bytes on 64-bit hosts. `keeps_values_at_three_words` now asserts
that size. List views store `u32` bounds, `CallableIdentity` is a thin `Rc`,
module-binding names are thin `Rc`s, and a declared native shares its identity
and resource signature behind one `Rc<ForeignDeclaration>`.

| Workload | Before | After |
|---|---:|---:|
| fc (5M calls) | 1.50 s | 1.24 s |
| gg (5M global reads) | 0.82 s | 0.65 s |
| bt (trees, depth 19) | 0.52 s | 0.44 s |
| fk (fannkuch 8) | 1.09 s | 0.96 s |
| ap (20k appends) | 0.66 s | 0.57 s |
| bt peak RSS | 68.7 MB | 52.5 MB |

`make check` passed. No diagnostic, call-frame, or scheduler behavior changed;
the CLI binary size is unchanged within 16 bytes.

#### Measurement record: move locals on last read (2026-10-12)

`liveness::mark_last_local_reads` runs when a chunk is installed, from either
`Program::add_chunk` or a decoded `.cslug` record, and sets packed operand `b`
on each final `GetLocal`. It recomputes the flag every time, so the flag never
comes from untrusted input. The VM moves a direct local out, leaving `nil`, and
still clones a captured cell. Focused tests cover an in-place append, an
earlier read that must clone, a read on only one branch, and a captured slot.

Interleaved A/B harness, seven rounds, against the 24-byte `Value` commit:

| Workload | Before | After |
|---|---:|---:|
| fc | 1.25 s | 1.22 s |
| gg | 0.65 s | 0.65 s |
| bt | 0.44 s | 0.43 s |
| fk | 0.96 s | 0.88 s |
| ap (20k appends) | 0.57 s | <0.01 s |

Typed `slug.db.sqlite` query cost was timed as the median of eleven runs: a
program that inserts N three-column rows and queries them, minus one that
only inserts. Results are in nanoseconds per row:

| Rows | Pre-sweep | 24-byte `Value` | Move on last read |
|---:|---:|---:|---:|
| 1,000 | 7,512 | 6,611 | 4,793 |
| 4,000 | 13,196 | 11,279 | 4,612 |
| 8,000 | 20,881 | 17,179 | 4,516 |

`collected + [row]` no longer copies the result list, so the per-row cost is
now flat. It remains about eight times the 0.13 prototype's 575 ns, mostly
native calls per column value. A per-row C helper is the next step if a
workload needs it. `make check` passed.

#### Measurement record: call-path fast exits (2026-10-12)

`Return` pops the frame directly when no defer scope, cleanup action, or
pending cleanup could observe it; otherwise it goes through the cleanup queue
as before. An exact positional `recur` moves its stack arguments straight into
direct locals. It falls back unchanged for captured locals, nested defer
scopes, unresolved bindings, or a malformed frame, and records the same
restart, argument-write, and local-vector-reuse metrics. The dispatch loop
keeps the active frame's `Rc<Program>` and refreshes it only when the top
frame's program changes.

Interleaved A/B harness, seven rounds, against the move-on-last-read commit:

| Workload | Before | After |
|---|---:|---:|
| fc | 1.22 s | 0.95 s |
| gg | 0.65 s | 0.50 s |
| bt | 0.43 s | 0.42 s |
| fk | 0.88 s | 0.79 s |

The existing defer, recovery, capture, and `recur` suites, together with
`records_direct_local_vector_reuse_across_recur`, cover the fallback
boundaries. `make check` passed. Item 10's per-instruction overhead remains
open for the single-dispatch rewrite.

#### Measurement record: global hashing and cold errors (2026-10-12)

The global environment is now a `platform::NameMap`, which hashes with FNV-1a
on hosted targets and stays a `BTreeMap` on freestanding ones. `Vm::error` and
`error_at` are `#[cold]` and `#[inline(never)]`. Interleaved A/B harness,
seven rounds, against the fast-exits commit:

| Workload | Pre-sweep | Before | After |
|---|---:|---:|---:|
| fc | 1.50 s | 0.95 s | 0.88 s |
| gg | 0.82 s | 0.51 s | 0.40 s |
| bt | 0.52 s | 0.41 s | 0.37 s |
| fk | 1.10 s | 0.79 s | 0.73 s |
| ap | 0.67 s | <0.01 s | <0.01 s |
| bt peak RSS | 68.7 MB | 52.6 MB | 52.5 MB |

`make bench-source` after the four sweep commits, with 15 samples, gave these
medians. CPython medians are from the same run; the pre-sweep Slug medians
were taken earlier the same day:

| Workload | Pre-sweep Slug | Slug | CPython | Slug / CPython |
|---|---:|---:|---:|---:|
| function-call | 34.312 ms | 20.376 ms | 27.221 ms | 0.75x |
| n-body | 14.416 ms | 9.214 ms | 22.898 ms | 0.40x |
| spectral-norm | 6.779 ms | 5.415 ms | 21.231 ms | 0.26x |
| binary-trees | 35.450 ms | 26.617 ms | 30.011 ms | 0.89x |
| fannkuch-redux | 117.653 ms | 80.314 ms | 31.917 ms | 2.52x |

`make check` passed. The release CLI binary is 32 bytes larger than before the
sweep.

#### Measurement record: wider source corpus (2026-10-12)

Before the dispatch-loop rewrite, the source corpus gained seven workloads
that the four landed commits were not tuned on (see
`docs/engineering/benchmarking.md`), and the spike's interleaved harness
became `make bench-ab`. Seven rounds, `master` (`a4ba29a`) against the four
sweep commits (`6d48f4a`):

| Workload | `master` | Sweep | Ratio |
|---|---:|---:|---:|
| function-call | 34.4 ms | 21.8 ms | 0.63x |
| n-body | 15.0 ms | 10.0 ms | 0.67x |
| typed-n-body | 14.8 ms | 9.7 ms | 0.66x |
| spectral-norm | 7.3 ms | 6.0 ms | 0.82x |
| typed-spectral-norm | 7.2 ms | 5.8 ms | 0.80x |
| binary-trees | 36.0 ms | 27.2 ms | 0.75x |
| fannkuch-redux | 115.9 ms | 81.4 ms | 0.70x |
| shared-append | 114.8 ms | 95.9 ms | 0.84x |
| higher-order | 335.7 ms | 60.2 ms | 0.18x |
| fib | 141.7 ms | 104.7 ms | 0.74x |
| defer-errors | 124.8 ms | 98.2 ms | 0.79x |
| word-frequency (hold-out) | 345.6 ms | 169.8 ms | 0.49x |
| string-building (hold-out) | 149.8 ms | 146.5 ms | 0.98x |
| expression-evaluator (hold-out) | 273.5 ms | 213.0 ms | 0.78x |

Geometric-mean ratio: tuned 0.65x, hold-out 0.72x. No workload regressed, so
the sweep generalizes beyond the workloads it was measured on. `higher-order`
gains most because `slug.std` `map` and `filter` build their results with
`acc :+ h` in `recur`, which now appends in place. `shared-append`, where the
move must not happen, still gains from the smaller `Value`. Only
`string-building` is flat: string concatenation was outside the sweep.

The same `make bench-source` run also shows how far the original corpus
understated the CPython gap. The original workloads range from 0.27x to 2.68x
CPython, but the new ones are 1.8x–6.0x: `string-building` 5.96x,
`word-frequency` 5.16x, `defer-errors` 2.83x, `expression-evaluator` 2.59x,
`higher-order` 2.21x, `fib` 1.96x, and `shared-append` 1.83x. `fib` against
`function-call` (0.81x) isolates ordinary call/return cost, which `recur`
loops bypass. Accept the dispatch-loop rewrite only on a `make bench-ab` run
whose hold-out geomean does not regress.

#### Measurement record: one packed dispatch loop (2026-10-12)

The rewrite landed in four steps, each accepted on its own `make bench-ab`
run against the previous step:

1. **One `PackedOpcode` match.** `execute_instruction` matches every opcode
   once and reads pooled metadata directly. The rich-`Op` fallback
   (`unpack_instruction` plus `execute_borrowed_span_op`, including every
   un-pooled arm that installation never emits) is gone, along with the
   `packed_direct_dispatches` and `rich_op_fallback_dispatches` metrics it
   needed. Large or rare operations live in `#[inline(never)]` helpers, and
   the rarest (global definition, overload combination, imports, struct
   schemas, tasks, `select`, scope cleanup) are also `#[cold]`. Tuned 0.95x,
   hold-out 0.98x, release binary 16.5 KB smaller. Unlike the spike's
   arm deletion, nothing regressed.
2. **No `span` parameters.** The 40 helpers whose `span: Option<&SourceSpan>`
   was always `None` lost it, about 600 call-site arguments in all. Only the
   native-call path, which can carry a host-supplied span, keeps one.
   Execution was unchanged (1.00x), as LLVM had already folded the constant.
3. **Inlined stack fast paths and a cached code slice.** A time profile of
   `fib` showed `pop_at` as a separate non-inlined call costing 13–17% of
   samples. `pop_at`, `pop_pair_at`, `peek_at`, `local_value`, and
   `take_local` are now `#[inline]`, with binding resolution and underflow in
   `#[cold]` helpers (tuned 0.95x, `fib` 0.86x). The dispatch loop then keeps
   the active chunk in a local, reloading it only when `frame_epoch` changes.
   `push_frame`, `pop_frame`, frame truncation and clearing, execution swaps,
   and `select` suspension bump the epoch. `next_instruction` is gone (tuned
   0.98x, hold-out 1.00x). The instruction pointer stays in the frame, since
   only jumps, `recur`, and cleanup write it.
4. **No owned `call`.** The root entrypoint call now goes through `call_at`
   after clearing `active_span`, so `main` still records no call site. The
   owned `call`, `bind_overload_arguments`, and `bind_call_arguments` twins
   are removed. Execution was unchanged.

Seven rounds, `5694f30` against the finished rewrite:

| Workload | Before | After | Ratio |
|---|---:|---:|---:|
| function-call | 22.9 ms | 19.6 ms | 0.85x |
| n-body | 10.7 ms | 9.6 ms | 0.90x |
| typed-n-body | 10.6 ms | 9.1 ms | 0.86x |
| spectral-norm | 6.3 ms | 5.8 ms | 0.91x |
| typed-spectral-norm | 6.4 ms | 5.8 ms | 0.90x |
| binary-trees | 27.8 ms | 25.6 ms | 0.92x |
| fannkuch-redux | 83.5 ms | 68.3 ms | 0.82x |
| shared-append | 98.6 ms | 96.1 ms | 0.98x |
| higher-order | 62.4 ms | 50.9 ms | 0.82x |
| fib | 109.4 ms | 91.2 ms | 0.83x |
| defer-errors | 101.6 ms | 81.7 ms | 0.80x |
| word-frequency (hold-out) | 187.1 ms | 170.2 ms | 0.91x |
| string-building (hold-out) | 151.2 ms | 152.4 ms | 1.01x |
| expression-evaluator (hold-out) | 223.2 ms | 193.0 ms | 0.86x |

Geometric-mean ratio: tuned 0.87x, hold-out 0.93x. `string-building` is
inside its spread; it is dominated by string concatenation, not dispatch. The
release binary fell from 1,859,200 to 1,842,608 bytes. `mod.rs` lost about
800 lines net. `make check` passed.

The same `fib` profile points at the next costs, all outside dispatch:
`Value` clone and drop are out-of-line calls; every call to a global function
hashes and compares its name (FNV plus `memcmp`, about 6–8% of `fib`), which
is the evidence item 7's compiler-resolved global slots was waiting for; and
`push_frame`/`pop_frame` swap the VM's `globals` `Rc` on every call.

With the span parameter gone, the `_at` suffix on VM helpers no longer meant
anything, so they were renamed without it: `call_at` is now `call`, `pop_at`
is `pop`, and so on. `current_waiter_at` became `blocking_waiter`, and the
pooled `import` and `select` wrappers were merged into the functions they
called. `error_at` keeps its name: it builds an error at the active span,
unlike `error`, which takes one explicitly.
