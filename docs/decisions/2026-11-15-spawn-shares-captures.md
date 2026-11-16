# Spawned tasks share what they capture

## Status

Accepted (2026-11-15). Amends
[Adopt a cooperative task runtime](2026-08-23-cooperative-task-runtime.md).

## Context

A spawned task snapshotted its immediate lexical bindings, those of the
function holding the `spawn`, while outer bindings and module variables stayed
live. Which a `var` was depended on where the `spawn` was written: directly in
the declaring function, or in an `if`, the task's writes were lost and it
missed the spawner's later ones; in a `nursery { }` body, which compiles to a
function, or in a nested function, they were shared; and a write through a
closure made outside the task reached the spawner either way. Nothing reported
the difference.

It hid real bugs. A done flag set by one task and polled by another looped
forever, and `var results = []` filled by spawned tasks stayed empty unless
the tasks were spawned inside a nursery. The repository's own spawn sites never
relied on the snapshot: they capture `val`s, channels, and module variables.

## Decision

`spawn` shares every binding it captures, as any closure does. Tasks still take
turns on one thread, so another task's assignment appears only while a task
waits. A task that needs the value a `var` had when it started binds it to a
`val` first. `task var` keeps a per-task copy of a module variable, and `fork`
keeps its snapshot of everything, with no writes to bindings outside it.

## Consequences

- There are two models, each simple: `spawn` is cooperative and shares;
  `fork` may run in parallel and sees a snapshot it cannot change.
- Programs that relied on a task not seeing the spawner's later write, or on
  the spawner not seeing the task's, change meaning. None did in this
  repository.
- A narrowed `var` can change while a task waits, as it already could across a
  call to a closure that assigns it; the checker does not yet invalidate the
  narrowing in either case, and the mistake is a checked runtime error.

## Migration

To keep a task's view of a value fixed, bind it to a `val` before `spawn`.
