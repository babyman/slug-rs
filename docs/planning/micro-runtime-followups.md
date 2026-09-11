# Micro Runtime Follow-up Plan

This document turns the review of `feature/micro` into an actionable work
queue. It covers correctness, availability, lifecycle safety, and maintenance
of the new default-enabled `concurrency` Cargo feature. It does not change the
source-language contract or the host-driven progress decision recorded in
[Host-driven VM progress](../decisions/2026-09-11-host-driven-vm-progress.md).

## Goals and invariants

- Native producers never execute or re-enter Slug code from their threads.
- Every blocking adapter drains available native ingress after a wake.
- A checked runtime failure leaves a `Vm` reusable for a later execution.
- `Vm::shutdown` prevents an active host-driven execution from continuing.
- Both sides of every `concurrency` conditional compile in the repository gate.
- Slim builds retain channel readiness, native ingress, and applicable stdin
  behavior while rejecting scheduler-only operations with checked errors.

## Implementation sequence

Complete the packages in order. Keep fixes independently reviewable, use the
narrow tests listed for each package while iterating, and run the release gate
in the final package.

### 1. Restore native ingress during scheduler-owned progress

- [ ] Compose `ProgressDriver::make_available_progress` into the blocking
  scheduler loop used by `Nursery::run_task` and `Nursery::settle`.
- [ ] Preserve the non-blocking contract of `Vm::poll` and
  `Vm::run_until_stalled`.
- [ ] Preserve the generation snapshot/recheck sequence so a producer event
  cannot be lost between the last drain and the condition-variable wait.
- [ ] Add a VM regression in which an explicit nursery body suspends on a
  native channel and receives a value published later by a foreign thread.
- [ ] Add the equivalent spawned-task/root-settlement regression.
- [ ] Cover producer closure after suspension and require the receiver to
  observe normal channel closure rather than hang or report a false blocked
  runtime.

**Exit:** delayed native send and close events resume scheduler-owned tasks;
the tests fail under the reviewed implementation and pass after the fix.

**Focused command:** `make test-vm`

### 2. Fully unwind blocked host executions

- [ ] Centralize host-execution cleanup so completion, failure, blocked
  detection, cancellation, and shutdown cannot clear different subsets of
  state.
- [ ] Remove the root waiter from every retained wait registration before
  discarding a blocked execution.
- [ ] Clear `suspension`, `resume`, `wait_registration`, `current_waiter`, and
  `host_execution` consistently.
- [ ] Cancel scheduler-owned tasks with the existing checked error when the
  `concurrency` feature is enabled.
- [ ] Add full and slim VM regressions that run an impossible channel wait,
  observe the checked blocked error, and then successfully execute a second
  program on the same `Vm`.
- [ ] Add a cleanup assertion showing that the abandoned channel/select waiter
  is not retained.

**Exit:** a blocked run neither poisons the next invocation nor retains its
wait graph in either feature configuration.

**Focused commands:**

```sh
cargo test --features metrics --test vm
cargo test --no-default-features --features metrics --test vm
```

### 3. Define shutdown for active host-driven execution

- [ ] Make `Vm::shutdown` cancel and release an active host execution, including
  task, suspension, and wait-registration state.
- [ ] Ensure `poll`, `run_until_stalled`, and `blocking_run` cannot continue
  Slug execution after shutdown.
- [ ] Choose one checked, documented post-shutdown result for progress calls;
  do not silently report ordinary stalling while live execution state remains.
- [ ] Add tests for shutdown before the first poll and after a native-channel
  suspension, in full and slim configurations.
- [ ] Verify native resources and module-loader state are still closed exactly
  once.

**Exit:** shutdown is a terminal lifecycle transition even when `start` or
`start_named` has created an in-flight execution.

**Focused commands:**

```sh
cargo test --features metrics --test vm
cargo test --no-default-features --features metrics --test vm
```

### 4. Put the Cargo feature matrix in the repository gate

- [ ] Add a named Make target for slim validation.
- [ ] Run tests and strict Clippy without default features.
- [ ] Include the independent `metrics` combination so conditional metric
  fields and arguments compile without `concurrency`.
- [ ] Add the slim target to `make check`, which is the documented local and CI
  handoff gate.
- [ ] Update `docs/engineering/testing.md` and contributor command summaries to
  identify the supported feature matrix and focused slim command.

The minimum matrix is:

| Configuration | Test expectation |
|---|---|
| Default (`concurrency`) | Full runtime and scheduler behavior |
| Default + `metrics` | Full runtime instrumentation |
| `--no-default-features` | Slim runtime and negative-cfg tests |
| `--no-default-features --features metrics` | Slim instrumentation and cfg composition |

**Exit:** the normal repository gate executes code guarded by both
`cfg(feature = "concurrency")` and `cfg(not(feature = "concurrency"))`.

### 5. Restore slim native-ingress integration coverage

- [x] Remove the module-wide `concurrency` gate from the stdin CLI tests.
- [x] Gate only individual cases that actually require tasks, nurseries, or
  timers.
- [x] Run delayed input, EOF closure, prompt sharing, and prompt-flush behavior
  against a slim CLI binary.
- [x] Keep at least one direct slim VM native-producer test in addition to the
  end-to-end stdin coverage.
- [x] If a stdin behavior genuinely cannot work without `concurrency`, resolve
  that implementation gap or narrow the documented slim-runtime promise; do
  not hide the mismatch with a broad test cfg.

**Exit:** the real native stdin producer proves the documented slim ingress
capability end to end.

**Focused command:** `cargo test --no-default-features --test cli stdin`

### 6. Release verification

- [ ] Run `cargo fmt --all -- --check`.
- [ ] Run strict Clippy for every supported feature combination.
- [ ] Run the full default and slim test suites.
- [ ] Run `make docs-check` and `git diff --check`.
- [ ] Update `changelog.md` for the user-visible runtime, lifecycle, and build
  gate fixes.
- [ ] Move this document to `docs/planning/completed/` only after every exit
  condition is satisfied.

**Completion gate:**

```sh
make check
cargo test --no-default-features
cargo test --no-default-features --features metrics
cargo clippy --all-targets --no-default-features -- -D warnings
cargo clippy --all-targets --no-default-features --features metrics -- -D warnings
git diff --check
```
