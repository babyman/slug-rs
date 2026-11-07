# Implementing `fork` and `task var`

## Purpose

Implement [parallel fork tasks](../decisions/2026-11-07-parallel-fork-tasks.md):
`fork { ... }` starts a task that sees a snapshot and may run on another
core; `task var` gives every task its own copy of a module variable. The
semantics land first, single-threaded, and threads come after, so every
phase ships something that works and the threaded phases change only speed.

## What exists today

- **Tasks are child VMs.** `Op::Spawn` (`vm/mod.rs`, `spawn_task`) pops a
  closure, snapshots its `Capture::Local` cells, shares its
  `Capture::Capture` cells, and builds a child `Vm` that shares the parent's
  `GlobalEnvironment` (`Rc<RefCell<GlobalTable>>`), `Nursery`, and
  `ProgressDriver`. A fork is the same shape with different capture and
  global handling, and a child VM is the natural unit to move to a thread.
- **Every module top-level name is a binding cell** (`Value::Binding`,
  `Rc<RefCell<Value>>`), and an importer holds the exporter's cells. Module
  declarations carry `mutable` (`ModuleDeclaration`).
- **Single-threaded caches in shared bytecode:** per-closure `GlobalSlots`
  (`Rc<[Cell<u32>]>`) and per-chunk `index_hints` (`Box<[Cell<u32>]>`).
- **Everything is `Rc`:** values, closures, programs, `NativeFunction`, the
  host (`Rc<dyn VmHost>`), nurseries, timers, channels.
- **The scheduler needs `std`.** `scheduler.rs` and `timers.rs` use
  `std::time::Instant`, and the `concurrency` feature, which gives `spawn`,
  `nursery`, and channels' waiting, is off on the ESP32-S3 firmware
  (`default-features = false`). Embedded builds have no tasks today.
- **The experiment branch** (`experiment/arc-values`) has the data-value
  `Shared<T>` switch, `Value::shareable`, and the snapshot census.

The record was clarified accordingly: `fork` exists wherever tasks do, and
bringing tasks to `no_std` (phase 5) is part of this work.

## Phases

Each phase lands on its own with tests green, a changelog entry, and the
ESP32 check. Phases 2 and 3 touch hot paths and are validated with the PGO
`bench-ab` against hold-outs.

### Phase 0: the semantics, as tests (done)

Write the behavior first, as VM and CLI conformance tests that every later
phase must keep passing on every build:

- a fork sees captures and globals as of the fork, not later changes;
- writing a captured `var` or a non-`task` module `var` in a fork fails,
  directly and through a called function, naming the variable and the fork;
- `task var`: every task, spawned or forked, starts with its spawner's
  current value, and its writes stay its own; the main task starts with the
  declared value;
- results and channel sends must be sendable; using a snapshotted task
  handle or native resource fails;
- `fork` during module initialization fails;
- calling a native function not declared thread-safe in a fork fails, and
  so does initializing a module from one;
- nursery membership, `await`, `select`, cancellation, and fail-fast behave
  as for `spawn`;
- channels work between a fork and its spawner, both ways.

Landed as `tests/conformance/fork/` (fixtures marked `pending` with their
phase) and two ignored VM tests in `crates/slug-vm/tests/vm/concurrency.rs`
for natives and resources. Writing them settled one more rule: a closure
that captures a `var` is not sendable, since the fork would otherwise see
its spawner's live cell through it.

### Phase 1: the language surface (frontend)

- **Lexer and parser:** `fork` becomes a reserved word (`TokenKind::Fork`)
  parsed like `spawn` into `ExprKind::Fork`. `task` is contextual before
  `var` at module level; `task val` and a `task var` in a function body are
  compile errors.
- **Metadata:** `ModuleDeclaration` gains `task: bool`; the clutch manifest
  gains a thread-safety declaration for native functions, set for the core
  clutch.
- **Type checker:** a fork's type is `task<T>`, as for `spawn`. At the fork
  site it reports what it can see: an assignment in the body to a captured
  function-local `var` or a non-`task` module `var`, a `fork` in module
  top-level code, and an unsendable result type.
- **Compiler:** `Op::Fork`.
- **Docs and tooling:** the language specification, `language-support.tsv`,
  the formatter, and the REPL's keyword list.

### Phase 2: cooperative fork and `task var` (VM, one thread)

The full semantics on one thread, under `concurrency`. After this phase
programs can use `fork` and agents can be evaluated on it; later phases
change only speed.

- **Per-task `var` cells.** At install, each module `var` binding gets a
  `VarId`. A task carries an overlay from `VarId` to its own cell: forks get
  an entry for every `var`, other tasks for `task var`s only, each filled
  with the spawner's current value. `val` bindings carry no id, so their
  reads are unchanged; a `var` read checks the overlay only when the task
  has one.
- **Fork captures.** A fork snapshots outer captures too, not only
  immediate ones. The cells it snapshots are marked, and assigning one in
  the fork fails.
- **The write rule.** A child VM knows it runs in a fork (inherited by
  tasks the fork spawns). `SetGlobal` on a non-`task` `var` and `SetCapture`
  on a snapshotted cell fail there with an error naming the variable, the
  write, and the fork's span.
- **Sendable values.** A domain id per fork: task handles and native
  resources record the domain that made them, and using one from another
  domain fails. Results and channel sends from or into a fork are checked
  for sendability by a scan that stops at leaves, so strings and bytes cost
  nothing.
- **Module initialization and imports.** `fork` while a module initializes
  fails at runtime too, and so does initializing a new module from a fork.
- **Natives.** Calling a native function not declared thread-safe in a fork
  fails.
- **Crawl, for correctness.** Mark Crawl's `task var`s and draw in forks
  under the cooperative runtime, so the write rule and the errors are tested
  on a real program before threads.

Measure: `bench-ab` with hold-outs for the `var`-read and spawn paths;
spawn's added `task var` copy grows with the number of `task var`s.

### Phase 3: thread-safe representation (`threads` feature)

`threads` implies `concurrency` and is the default on multi-core desktop
targets. No scheduling changes yet: forks still run cooperatively, so this
phase is measured on its own.

- **Shared counts.** Port the experiment's `Shared<T>` (`Arc` under
  `threads`, `Rc` otherwise) to data values, closures, programs, channels,
  schemas, and `NativeFunction`.
- **`val` cells read across threads.** Shared `val` cells must be `Sync`.
  The candidates: a write-once cell (`OnceLock`-like), with REPL
  redefinition replacing cells instead of writing them; or an atomically
  swapped pointer, which keeps redefinition in place. This is decided here
  by measurement.
- **Caches.** `GlobalSlots` and `index_hints` become `AtomicU32` with
  relaxed loads and stores, which cost the same as `Cell` on x86 and ARM.
- **Host.** `VmHost` becomes `Send + Sync` under `threads`, so a fork can
  call declared thread-safe natives and import initialized modules from its
  worker.

Measure: `bench-ab`, `threads` against no `threads`, with hold-outs. The
budget is the experiment's 1.03x geomean.

### Phase 4: worker threads (desktop and server)

- **Pool.** One worker per available core. A fork's child VM moves to a
  worker and runs there until it settles or parks; tasks a fork spawns run
  cooperatively on its worker, sharing its snapshot.
- **Channels.** Under `threads`, a channel's state sits behind a lock, and
  its waiters are wakers into the scheduler that owns the waiting task. The
  timer service is shared or per worker.
- **Nursery and cancellation.** A fork's settlement crosses back to its
  nursery's owner through a queue. Cancellation becomes an atomic flag, read
  at calls and backward jumps as today.
- **Testing.** Phase 0's tests on many threads, stress tests for channel
  races and cancellation, and ThreadSanitizer runs in CI.

Measure: experiment 2 rerun with real `fork` (the target is 14.5 s on one
thread to about 3.7 s on eight); channel throughput with and without
`threads`.

### Phase 5: embedded multi-core

- **Tasks on `no_std`.** Move the scheduler and timers off
  `std::time::Instant` onto the platform clock, and progress waiting onto
  the host. This gives the ESP32-S3 `spawn` as well, and can land
  separately.
- **Threads on the ESP32-S3.** Start the second core's worker from the
  firmware and drive each worker with `Vm::poll`. Single-core targets build
  without `threads`.

### Phase 6: Crawl and evaluation

Draw Crawl's pages in forks on threads and measure the 1000-player server
against the experiment's numbers. Add an agent evaluation task that needs
parallel work, and triage what agents get wrong with `fork`, `task var`, and
the write errors.

## Decisions (2026-11-07)

1. **Native functions in forks** declare thread safety in the clutch
   manifest; calling an undeclared one in a fork is an error on every build.
   The core clutch is declared. Phase 1 adds the manifest field and phase 2
   the check.
2. **Imports in a fork** of an initialized module are allowed; initializing
   a new module in a fork is an error (phase 2).
3. **Embedded** (phase 5) is part of this work.
4. **Order** as written: cooperative semantics (phase 2) before the
   threaded representation and scheduler (phases 3 and 4).
