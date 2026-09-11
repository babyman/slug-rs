# Slim VM Follow-up Task List

## Goal

Finish the slim-runtime cleanup without changing Slug language semantics.

The guiding rule is:

> **Cargo features may change runtime capabilities and runtime representation, but must not change what constitutes
valid Slug source.**

A full and slim build should therefore:

- parse the same source,
- resolve the same names,
- infer the same types,
- produce the same type-checking diagnostics,

while differing only when execution reaches an unavailable runtime capability.

---

# 1. Remove Runtime Feature Awareness from the Type Checker

## Task 1 — Remove `#[cfg(feature = "concurrency")]` from `typecheck.rs` ✓

Remove the concurrency feature gate currently associated with runtime `Value::Task` handling.

The type checker should not depend on the runtime build configuration.

### Required behavior

Both:

```bash
slug-full --check program.slug
slug-slim --check program.slug
```

must produce identical results.

Source constructs such as:

```slug
spawn {
    work()
}
```

and task-aware `select` forms must remain:

- parseable,
- resolvable,
- type-checkable,

in the slim configuration.

They should fail only if execution reaches an unsupported runtime operation.

---

## Task 2 — Decouple Runtime `Value` Representation from Source Typing ✓

Inspect the current `value_type()` path and determine why source type checking requires matching against runtime
`Value`.

Prefer one of these outcomes, in order:

### Preferred

If `Value::Task` cannot legitimately occur in the compile-time values handled by `value_type()`, remove task handling
from that path entirely.

Task types should instead arise from source semantics:

```text
spawn expression
    ↓
Type::Task(T)
```

rather than:

```text
runtime Value::Task
    ↓
Type::Task(T)
```

### Acceptable fallback

If runtime-value-to-type conversion is genuinely required, move that conversion outside the source type checker into a
runtime-neutral helper.

Any necessary `#[cfg(feature = "concurrency")]` should then reflect only the physical absence of a runtime enum variant,
not alter type-checking behavior.

### Acceptance criteria

- no Cargo feature checks inside semantic type-checking logic,
- `Type::Task` remains available in both configurations,
- slim and full builds produce identical compile-time diagnostics.

---

# 2. Complete Mechanical Concurrency Extraction

## Task 3 — Gate `TaskExecution` as a Whole ✓

`TaskExecution` is concurrency-only state and should not exist in the slim runtime.

Instead of compiling the structure in both builds and gating individual fields or methods:

```rust
pub(crate) struct TaskExecution {
    vm: Vm,
    program: Rc<Program>,
    #[cfg(feature = "concurrency")]
    settle_nursery: bool,
}
```

gate the entire abstraction:

```rust
#[cfg(feature = "concurrency")]
pub(crate) struct TaskExecution {
    vm: Vm,
    program: Rc<Program>,
    settle_nursery: bool,
}
```

Gate its implementation as a unit as well.

### Acceptance criteria

The slim build contains no `TaskExecution` type or associated task-execution implementation.

---

## Task 4 — Continue Removing Concurrency-Only Dead Code ✓

Use the no-default-features build and compiler warnings to identify any remaining implementation that exists solely for:

- task scheduling,
- task execution,
- nursery admission,
- scheduler ready queues,
- task wake/resume bookkeeping.

Apply the rule:

> If code exists only to schedule, admit, suspend, resume, or coordinate Slug tasks, it belongs behind `concurrency`.

Do not gate shared channel, ingress, or host-progress machinery merely because it was historically used by the
scheduler.

### Acceptance criteria

```bash
cargo test --no-default-features
```

remains green and concurrency-only dead-code warnings are eliminated or meaningfully reduced.

---

# 3. Protect the Host-Driven Runtime Contract

## Task 5 — Add/Confirm Non-Blocking `run_until_stalled()` Tests ✓

Protect the core invariant:

> **`run_until_stalled()` never performs an OS wait.**

Tests should cover at least:

- empty native-backed channel,
- native producer that has not yet produced data,
- pending timer in the full runtime,
- channel `select` with no currently ready branch.

The call must return `Stalled` rather than waiting.

---

## Task 6 — Protect Against Native Re-entry ✓

Add or retain a test demonstrating that native producer notification:

- enqueues ingress,
- signals progress,
- does not directly execute Slug,
- does not resume a task on the producer thread.

Slug execution should occur only when the host later calls:

```text
poll()
```

or:

```text
run_until_stalled()
```

### Architectural invariant

> Only the host-driving thread executes or mutates VM-owned Slug state.

---

## Task 7 — Keep Progress Notification Scheduler-Neutral ✓

Confirm the generic progress signal remains independent of:

- `Task`,
- `Nursery`,
- timer scheduling,
- scheduler queues.

Its semantic contract should remain:

> **Something changed; polling the VM may now make progress.**

It must not encode:

> Resume this particular task.

---

# 4. Preserve the Shared Channel Boundary

## Task 8 — Keep Channels Available Without `concurrency` ✓

Verify the slim runtime continues to support:

- channel creation,
- channel closure,
- channel send/receive behavior required by the host-pump model,
- native channel producers,
- native ingress draining,
- channel readiness selection.

Do not move these behind the `concurrency` feature.

### Required architecture

```text
native producer
    ↓
thread-safe ingress
    ↓
progress notification
    ↓
host-driven VM
    ↓
core channel state
```

This path must remain functional without the scheduler.

---

## Task 9 — Keep Scheduler-Dependent `select` Cases Isolated ✓

Retain the current useful split:

### Shared/runtime-core

- channel receive,
- channel send,
- default.

### Concurrency-only

- timer/`after`,
- task `await`.

The existence of scheduler-specific `select` cases must not cause all of `select` to become a concurrency feature.

---

# 5. Runtime Capability Errors

## Task 10 — Standardize Unsupported Capability Errors ✓

Executed concurrency-only operations in a slim runtime should produce a checked, consistent runtime error.

Examples include:

- `spawn`,
- nursery operations,
- task await,
- timer-select operations.

Prefer one central representation such as:

```text
RuntimeCapabilityUnavailable("spawn")
```

rather than unrelated error strings at each call site.

### Acceptance criteria

Unsupported functionality:

- parses,
- compiles,
- type-checks,
- fails only when executed.

---

## Task 11 — Keep FFI ABI Stable Across Builds ✓

Confirm the slim configuration does not structurally alter the public FFI ABI.

Unavailable runtime capabilities should return stable machine-readable unsupported results rather than:

- missing symbols,
- null function pointers used as feature detection,
- alternate headers,
- different ABI layouts.

Native channel producer functionality remains supported in slim builds.

---

# 6. Embedding API Follow-up

## Task 12 — Document `Idle` vs `Stalled` as a Deferred API Question ✓

Do not necessarily change the API now, but document the semantic distinction between:

```text
Stalled
    active execution exists but cannot currently progress
```

and:

```text
Idle
    no active host execution exists
```

Currently, polling a VM with no active execution may also produce `Stalled`.

This is acceptable for the current API but could become ambiguous for:

- REPL sessions,
- long-lived embedded VMs,
- repeated program execution,
- interactive hosts.

Treat this as a future embedding-API refinement rather than a blocker for the slim-runtime work.

---

## Task 13 — Preserve Internal Stall Information ✓

Continue retaining enough internal information to eventually distinguish:

```text
ExternalWait
ScheduledWait
Quiescent
```

even if the public API still exposes only:

```text
Stalled
```

Do not attempt full deadlock detection as part of this cleanup.

---

# 7. Validation

## Task 14 — Run the Full Configuration Matrix ✓

Validate at least:

```bash
cargo test
cargo test --no-default-features
make check
```

and both optimized builds.

If practical, also run the shared VM benchmark in both configurations to ensure the follow-up cleanup has not regressed
the previously measured behavior.

---

## Task 15 — Add a Compiler-Equivalence Test ✓

Add a small fixture suite that is checked under both full and slim configurations.

Include source containing:

- `spawn`,
- task types,
- nursery syntax,
- task-await `select`,
- timer `select`,
- ordinary channels.

Assert that compile/type-check results are identical between configurations.

Execution tests may then assert the expected capability difference.

This is the strongest guard against accidentally turning slim Slug into a separate language dialect.

---

# Completion Criteria

This follow-up is complete when:

- the type checker contains no runtime-feature-dependent semantics,
- full and slim builds accept and reject identical Slug source,
- `TaskExecution` and other genuinely scheduler-only state disappear from the slim build,
- channels and native ingress remain shared,
- `run_until_stalled()` remains strictly non-blocking,
- native producers cannot re-enter Slug,
- unsupported concurrency features fail only during execution,
- both build configurations pass the full test suite.

The resulting boundary should be:

```text
                     Slug language
                          │
              parser / compiler / checker
                          │
                  identical semantics
                          │
                   host-driven VM
                 channels + ingress
                          │
              ┌───────────┴───────────┐
              │                       │
          slim runtime         concurrency runtime
                                      │
                             Task / Nursery / Timer
```

The defining principle remains:

> **The compiler describes Slug. The runtime build describes which execution capabilities are available.**
