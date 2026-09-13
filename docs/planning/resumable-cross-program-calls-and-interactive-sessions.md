# Resumable Cross-Program Calls and Interactive Session Supervision

## Purpose

Remove the VM assumption that an execution has one current `Program`, then make
interactive sessions host-owned supervisors of independently resumable
submitted-cell tasks. This implements
[the frame-owned-program decision](../decisions/2026-09-13-frame-owned-programs.md).

## Architectural target

```text
Host session
  ├─ committed bindings + compiler snapshot
  ├─ output and protocol routing
  ├─ submitted task A: listener, suspended in a function from an earlier cell
  ├─ submitted task B: send, completed
  └─ submitted task C: later expression

Task execution
  └─ frames
      ├─ program: interactive cell 4
      ├─ program: interactive cell 2
      └─ program: slug.channel module
```

The governing invariant is: an execution has a current frame, never a current
program. Every frame owns the program/code unit used for instruction, chunk,
source span, and call metadata lookup.

`Session` remains a host-level owner of ordinary tasks. It is not a Slug task
kind and does not introduce a REPL-specific execution engine.

## Non-goals

- TCP transport, a server daemon, named channels, and cross-process sharing.
- A public compatibility promise for private bytecode, frames, programs, or
  task layout.
- Transactional rollback of mutations to already shared values or host effects.
- `TaskKind::Interactive` or another terminal-specific VM task variant.

## Required outcomes

1. A Slug closure from any program/module/cell can suspend and resume in its
   caller's task; no cross-program call creates a nested synchronous VM.
2. Imported `slug.channel.recv` and closures retained from prior REPL cells can
   wait without producing `task remains blocked with no runnable work` solely
   because of their program origin.
3. A waiting submitted task does not prevent a later binding-free task in the
   same session from running and waking it.
4. New top-level bindings become visible only after their cell settles
   successfully. A binding-producing pending cell blocks later declarations;
   binding-free work may proceed.
5. Closing a session cancels and releases every session-owned task. Background
   failures remain observable through a structured request or poll diagnostic.

## Tasks

### Task 0 — Regression suite and baseline

- [x] Add VM coverage for a cross-program closure that blocks on a channel and
  later resumes.
- [x] Add source coverage for imported `slug.channel.recv` and `send` waiting
  and resuming.
- [x] Add interactive server and REPL coverage for a prior-cell closure,
  imported channel calls, listener/send resumption, background failure, and
  session-close cancellation.
- [x] Capture each current blocked-task failure as a reproducer, not an
  expected result.

**Gate:** every reproducer has an expected value, output, or diagnostic. The
expected-final-behavior regressions are intentionally ignored until Task 2.

### Task 1 — Program-owned frames

- [x] Add an owned `Rc<Program>` or equivalent stable code reference to `Frame`.
- [x] Make instruction fetch, chunk lookup, span lookup, cleanup, call-frame
  rendering, and validation derive their program from the active frame.
- [x] Retain a root-entry owner only for installation/startup; stop using a VM
  or task-level program as dispatch authority.
- [x] Audit frames created by ordinary source, imports, modules, interactive
  cells, spawned tasks, deferred actions, and cleanup.

**Gate:** same-program behavior and malformed-bytecode diagnostics remain
unchanged.

### Task 2 — Normal frame calls across programs

- [x] Remove `call_module_closure` and `run_nested_execution` as the
  cross-program closure path.
- [x] Make ordinary calls, `call_at`, overloads, pipelines, spreads, deferred
  actions, and callbacks push a normal frame for any Slug closure.
- [x] Preserve closure globals/captures, arity checks, call spans, selected
  overload identities, `return`, `throw`, `defer`, `recur`, and stacktraces.

**Gate:** Task 0 cross-program VM and source cases pass without a nested VM.

### Task 3 — Program-polymorphic task execution

- [x] Remove remaining `TaskExecution` single-program assumptions.
- [x] Prove mixed-program frame stacks survive select/channel/timer/task-await
  suspension, cancellation, native ingress, and task settlement.
- [x] Keep nursery ownership and scheduler admission unchanged.

**Gate:** a spawned task can suspend inside an imported blocking function and
settle after an ordinary sender runs.

### Task 4 — Submitted-cell binding overlays

- [x] Give each cell an uncommitted binding overlay layered over committed
  session bindings and host bindings.
- [x] Compile later cells from only the committed compiler snapshot.
- [x] Promote bindings and compiler state after successful settlement; discard
  uncommitted bindings after source/runtime failure.
- [x] Define assignment to existing mutable bindings and retain ordinary shared
  object/channel side effects.
- [x] Retain the conservative declaration gate while a binding-producing cell
  is pending.

**Gate:** `var x = blocking()` does not expose `x` to another cell, while a
previously committed `msg` can wake the blocked cell.

### Task 5 — Session task supervision

- [x] Replace the single active execution slot with a session-owned task set.
- [x] Pump all runnable session tasks after submit/poll while retaining request
  result attribution and session output attribution.
- [x] Define deterministic background-failure delivery.
- [x] Cancel and release every owned task at session close.
- [x] Either implement the same contract for slim executions or explicitly
  retain and document its limitation as a separate parity task.

**Gate:** listener/send REPL regressions pass; independent sessions remain
isolated.

The slim runtime retains its existing single-submission session behavior:
while a slim submission is active, callers must poll it before submitting more
source. Multi-task session supervision depends on the concurrency scheduler and
is intentionally a separate parity task.

### Task 6 — Simplify and document

- [ ] Delete superseded nested-execution and single-program code paths.
- [ ] Update runtime requirements, README, changelog, and decisions without
  rewriting historical records.
- [ ] Run the full validation matrix below.

**Gate:** no Slug closure uses a nested VM merely because its program differs
from its caller's.

## Review checkpoints

| After | Confirm |
|---|---|
| Task 2 | Frame-owned program identity preserves calls, diagnostics, and cleanup. |
| Task 4 | Binding isolation does not claim rollback of shared-object side effects. |
| Task 5 | Background errors, polling, output order, and slim parity have a public contract. |

## Validation

Use focused tests while iterating. Before the final handoff, run:

```sh
make test-vm
make test-cli
cargo test --features metrics --test module_loader
cargo test --features metrics --test interactive_server
cargo test --features metrics --test interactive_repl
make check
```

All invalid source must remain `SourceError`; runtime failures must remain
`RuntimeError`; protocol failures must remain structured diagnostics. No
cross-program suspension may panic or block an otherwise non-blocking VM pump.
