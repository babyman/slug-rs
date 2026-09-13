# Interactive Retained Cells for the Slim Runtime

## Goal

Make interactive sessions have one observable execution model in both runtime
configurations. A complete top-level form is an interactive **cell**. A cell
that suspends remains session-owned while later binding-free cells may run and
wake it. The default runtime carries a retained cell in a scheduler task; the
slim runtime carries it in a detached `InteractiveExecution`.

This closes the slim-runtime parity gap that currently returns
`submission_active` after a retained receive, preventing a later
`send(msg, value)` from waking it.

The source-language and protocol contract is in
[runtime requirements](../language/runtime-requirements.md). This plan extends
the completed [interactive supervision plan](completed/resumable-cross-program-calls-and-interactive-sessions.md)
without revising its historical record.

## Architectural model

```text
Session
  ├─ committed environment + compiler snapshot
  ├─ retained cell A ── scheduler task          (default runtime)
  ├─ retained cell B ── detached execution      (slim runtime)
  └─ output and protocol routing
```

The concepts must remain separate:

| Concept | Owns |
|---|---|
| Session | namespace, compiler snapshot, protocol lifecycle, output attribution |
| Cell | one compiled top-level form and its settlement/commit eligibility |
| Execution | resumable VM frames, stack, waits, and continuation state |

Scheduler tasks and detached executions are carriers for a cell, not distinct
interactive semantics.

## Required invariants

- Both builds compile a complete submission into ordered top-level cells.
- Completed cells commit their declarations and compiler state before the next
  form starts.
- A retained cell retains its uncommitted declarations until successful
  settlement.
- While any retained cell exists, a later cell that declares bindings is
  rejected with `background_bindings`.
- A later binding-free cell is accepted and may wake a retained cell through
  previously committed values such as channels.
- Session overlays isolate namespace changes, but preserve the identity of
  committed mutable `BindingCell`s. They must not copy mutable binding storage.
- Closing a session cancels every retained cell and removes all waits.
- Output remains attributed to the submitting session and is emitted before its
  associated response or diagnostic.

## Non-goals

- Allowing concurrent binding-producing cells or defining their commit order.
- Introducing scheduler tasks into a slim build.
- Introducing a public VM execution, frame, or bytecode compatibility API.
- Rolling back mutations to already committed mutable cells, channels, host
  resources, or other shared effects after a later cell fails.

## Tasks

### Task 0 — Capture parity regressions

- [x] Add feature-matrix server coverage for the same stalled-cell scenario
  with default features and `--no-default-features`.
- [x] Reproduce a startup source that commits `msg`, stalls in `recv(msg)`, and
  resumes after a later `send(msg, value)`.
- [x] Cover protocol results, output events, and binding visibility rather than
  terminal prompt formatting alone.

**Gate:** default and slim tests capture the same desired wakeup transcript.

### Task 1 — Normalize source into ordered interactive cells

- [x] Remove the slim-only whole-submission compilation path.
- [x] Make both server configurations use `compile_interactive_forms` and
  preserve source order within a complete submission.
- [x] Commit each completed preceding form before compiling/running its next
  form, including startup source passed through `slug-repl session.slug`.
- [x] Retain source-readiness behavior: incomplete source remains pending;
  parse and semantic failures clear only that pending source and commit no new
  state.

**Gate:** the startup program below leaves only `f()` retained; `msg` and `f`
are available to later cells.

```slug
val {*} = import('slug.channel')
var msg = chan(8)
val f = fn() { recv(msg) /> println('received'); recur() }
f()
```

### Task 2 — Model retained cells independently of carriers

- [x] Replace slim `SessionExecution::{Idle, Active, Stalled}` with retained
  cell records equivalent in lifecycle to the default runtime's retained task
  submissions.
- [x] Give each record its compilation and one carrier:
  `InteractiveTask` in default builds or `InteractiveExecution` in slim builds.
- [x] Keep the carrier representation private and avoid a public trait or task
  kind unless a shared helper cannot express the lifecycle cleanly.
- [ ] Define small internal lifecycle operations: start, drive, settle, retain,
  and cancel.

**Gate:** a session can own multiple retained binding-free cells without either
  build replacing or losing a previously suspended continuation.

### Task 3 — Share committed bindings without exposing pending namespaces

- [x] Factor overlay creation so default and slim cells clone the committed
  namespace map while retaining shared `BindingCell` identities.
- [x] Factor settlement synchronization so completed declarations/imports are
  merged into the committed session environment in both builds.
- [x] Rebind committed closures to the durable session globals, including
  overload sets, so later calls observe session mutations.
- [x] Keep pending-cell declarations absent from later compiler snapshots and
  runtime lookup.
- [x] Preserve immediate visibility of mutations to bindings committed before
  the retained cell began.

**Gate:** this scenario prints `1` under both feature configurations:

```slug
val {*} = import('slug.channel')
var n = 0
var gate = chan(1)
val f = fn() { recv(gate); n = n + 1 }
f()
send(gate, true)
println(n)
```

### Task 4 — Accept and pump later binding-free cells

- [x] Replace slim's unconditional `submission_active` rejection with the
  shared `background_bindings` declaration gate.
- [x] Drive the submitted binding-free cell and every runnable retained cell
  after `submit` and `session.poll`.
- [x] Preserve non-blocking host progress: slim must use available native and
  timer progress only and must not introduce scheduler waits.
- [x] Return `stalled` while retained work remains, `idle` after all retained
  cells settle, and a structured runtime diagnostic for a background failure.
- [x] Preserve event-before-response ordering when a send wakes a retained
  receiver that prints output.

**Gate:** after the startup program in Task 1 stalls, `send(msg, 2)` is
accepted, emits `received`, and leaves the receiver retained for its next
message.

### Task 5 — Teardown and error isolation

- [x] Cancel every retained slim execution during `session.close`, removing
  channel, task-await, and timer wait registrations.
- [x] Discard failed cell declarations while retaining effects on previously
  committed mutable cells and explicit shared resources.
- [x] Ensure one session's retained cells cannot be progressed, committed,
  cancelled, or attributed to another session.
- [x] Verify server shutdown releases all retained slim contexts without host
  panics.

**Gate:** closing a session with several waiting cells leaves no waiter that a
later send or timer can observe.

### Task 6 — Simplify and document

- [ ] Delete the superseded single-active-submission state and its
  `submission_active` protocol behavior.
- [ ] Update the README and runtime requirements only where they still imply a
  build-specific interactive restriction.
- [ ] Add an implementation decision record only if the final carrier boundary
  changes the established runtime architecture beyond this plan.
- [ ] Add the user-visible behavior change to `changelog.md`.

**Gate:** default and slim tests share the same interactive protocol assertions
without feature-specific expected outcomes.

## Review checkpoints

| After | Confirm |
|---|---|
| Task 1 | Startup source commits preceding forms and retains only the blocked form. |
| Task 3 | Overlays share mutable binding identity but hide pending declarations. |
| Task 4 | Both carriers accept a later binding-free sender and preserve output ordering. |
| Task 5 | Teardown removes every retained wait and preserves session isolation. |

## Validation

Run focused checks during implementation:

```sh
cargo test --features metrics --test interactive_server
cargo test --no-default-features --features metrics --test interactive_server
cargo test --features metrics --test interactive_repl
cargo test --no-default-features --features metrics --test interactive_repl
make test-vm
```

Before handoff, run:

```sh
make check
```

All invalid source must remain `SourceError`, runtime failures must remain
`RuntimeError`, and protocol failures must remain structured diagnostics. No
slim interactive operation may block the host thread or require a scheduler.
