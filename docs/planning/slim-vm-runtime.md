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

- [x] Retain channel send/receive select in the slim runtime.
- [x] Keep timer and task-await select registration, state, and metrics behind
  `concurrency`.
- [x] Verify that ready/default channel select behavior remains unchanged.

**Exit:** slim select owns only channel readiness and root suspension state.

### 4. Make the slim configuration warning-clean

- [x] Feature-gate concurrency-only VM tests, imports, and fixtures.
- [x] Pass `cargo clippy --no-default-features -- -D warnings`.
- [x] Preserve host-pump and native-ingress tests under both configurations.

**Exit:** `cargo test --no-default-features` and strict slim Clippy both pass
without warnings.

### 5. Measure the extracted runtime

- [x] Compare full and slim binary size using the same target/profile.
- [x] Compare VM layout and baseline allocation/initialization costs.
- [x] Record the commands, environment, and results beside the change or in a
  focused planning note; do not introduce timing-sensitive assertions.

**Exit:** the repository has reproducible evidence showing whether the feature
split materially benefits embedding.

### Measurement record: 2026-09-11

Measurements used Rust 1.96.1 on `aarch64-apple-darwin` (`arm64`) and the
optimized Cargo release profile. Each comparison was built from the same
checkout with the Cargo lockfile:

```sh
cargo build --release
stat -f '%z' target/release/slug
cargo build --release --no-default-features
stat -f '%z' target/release/slug
```

| Measurement | Full (`concurrency`) | Slim (`--no-default-features`) | Change |
|---|---:|---:|---:|
| `slug` binary bytes | 2,663,984 | 2,604,752 | -59,232 (-2.2%) |
| `size_of::<Vm>()` | 568 | 504 | -64 (-11.3%) |
| `Closure` layout bytes | 72 | 48 | -24 (-33.3%) |
| `Value` layout bytes | 48 | 48 | unchanged |
| `Frame` layout bytes | 152 | 152 | unchanged |

The VM and closure values are inline-layout baselines; their owned heap
allocations are intentionally excluded. A single warm process measurement of
`slug --version` with `/usr/bin/time -l` reported maximum resident sizes of
1,654,784 bytes (full) and 1,638,400 bytes (slim), a 16,384-byte reduction.
This process-level observation is directional only and is not a timing or
memory regression threshold.

The slim configuration therefore produces a modest but concrete embedding
benefit: scheduler task state and its closure metadata disappear, while the
shared value and frame representations remain the same.

### 6. Close documentation

- [ ] Update the architecture note and changelog only for findings that alter
  user-visible capability, error behavior, or documented embedding guidance.
- [ ] Move this plan to `docs/planning/completed/` when all extraction and
  measurement work is complete.
