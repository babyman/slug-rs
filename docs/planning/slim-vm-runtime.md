# Slim VM Runtime Plan

This document tracks the remaining mechanical extraction work for the optional
`concurrency` runtime. It is an implementation checklist, not a source-language
specification or a new compatibility policy. The host-driven progress model is
recorded in [Host-driven VM progress](../decisions/2026-09-11-host-driven-vm-progress.md).

## Current boundary

The VM has a default-enabled `concurrency` Cargo feature. Native ingress,
channels, progress signals, `poll`, and `run_until_stalled` are available in
both configurations. The blocking `run` APIs remain adapters over that host
progress model.

The scheduler and timer modules are now compiled only with `concurrency`.
Source programs may still contain concurrency syntax in a slim build; executing
an unavailable operation returns a checked runtime capability error.

## Remaining implementation sequence

Complete one item at a time. Each item must preserve full-runtime behavior,
keep the slim source/compiler contract intact, run focused tests in both
feature configurations where applicable, and pass `make check` before handoff.

### 1. Gate task machinery

- [x] Compile `Task`, `TaskState`, task-specific `TaskExecution` behavior,
  admission, settlement, cancellation, and ready-queue coordination only with
  `concurrency`.
- [x] Preserve the shared root/channel suspension path required by the slim
  host pump.
- [x] Remove the corresponding slim-build dead-code warnings.

**Exit:** `scheduler.rs` is already absent from the slim build, and no task
state or task-coordination implementation remains reachable there.

### 2. Remove task values from the slim runtime

- [x] Compile `Value::Task` and task-only value matching/formatting only with
  `concurrency`.
- [x] Keep task source forms valid through parsing and type checking.
- [x] Retain checked unavailable-capability errors when slim execution reaches
  `spawn`, nursery, task await, or task-await select.

**Exit:** the slim value representation has no task variant while the language
front end continues to accept the same source.

### 3. Finish select feature boundaries

- [ ] Retain channel send/receive select in the slim runtime.
- [ ] Keep timer and task-await select registration, state, and metrics behind
  `concurrency`.
- [ ] Verify that ready/default channel select behavior remains unchanged.

**Exit:** slim select owns only channel readiness and root suspension state.

### 4. Make the slim configuration warning-clean

- [ ] Feature-gate concurrency-only VM tests, imports, and fixtures.
- [ ] Pass `cargo clippy --no-default-features -- -D warnings`.
- [ ] Preserve host-pump and native-ingress tests under both configurations.

**Exit:** `cargo test --no-default-features` and strict slim Clippy both pass
without warnings.

### 5. Measure the extracted runtime

- [ ] Compare full and slim binary size using the same target/profile.
- [ ] Compare VM layout and baseline allocation/initialization costs.
- [ ] Record the commands, environment, and results beside the change or in a
  focused planning note; do not introduce timing-sensitive assertions.

**Exit:** the repository has reproducible evidence showing whether the feature
split materially benefits embedding.

### 6. Close documentation

- [ ] Update the architecture note and changelog only for findings that alter
  user-visible capability, error behavior, or documented embedding guidance.
- [ ] Move this plan to `docs/planning/completed/` when all extraction and
  measurement work is complete.
