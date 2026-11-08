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

### Phase 1: the language surface (frontend) (done)

Landed with these choices: `Op::Fork` reports "not implemented" until
phase 2; the write rule and the module-initialization rule are a lexical
pass (`semantics/forks.rs`), and only the result check uses types; `.cslug`
stores a `task var` as mutability byte 2, so existing artifacts still read;
`thread_safe` is per function, not per clutch, since core's stdin reader and
signal registration are not. The REPL, which compiles each entry as module
top-level code, runs the fork pass without the initialization rule (it ran
no fork checks before), so `fork` may be typed at the prompt; phase 2's
runtime rule must likewise not treat an interactive entry as module
initialization.


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

### Phase 2: cooperative fork and `task var` (VM, one thread) (done)

Landed in four steps (fork tasks; module `var` snapshots and `task var`;
sendability; natives, imports, and initialization), with these choices:

- **Views.** A task VM carries an optional `TaskView`: its fork context (a
  domain id, the capture cells it snapshotted, its snapshot of every other
  `var`) and its own copy of each `task var`. Ordinary tasks have none, so a
  global read outside forks pays one branch. A nursery body shares its
  task's view.
- **Registry.** Each host shares a `ModuleVariables` registry, like its
  native-resource registry, which also counts initializing modules and gives
  fork domains. A module binding's kind lives in a shared `BindingName`, so
  `Value` stays three words and importers see the kind set at install.
- **Domains** stamp closures and task handles; using one from another
  domain fails. Channels are marked once a fork uses them, and every later
  send on one is checked; a fork's immediate receives and its result are
  checked by a scan that stops at leaves.
- **Captures.** Chunks record each capture's name and mutability, so a fork
  can refuse a closure made outside it that captures a `var`.

Known gaps, for later phases: `.cslug` does not yet carry capture details
(decoded closures count as capturing no `var`; phase 5 adds them with a
compatibility bump); a native resource made outside a fork is caught when
returned or sent, but not when passed to a native declared thread-safe
(none takes a resource today); REPL session variables are not registered, so
a fork at the prompt reads them live; a value a woken fork receives is not
checked, only sends on channels a fork has used.


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

Progress (2026-11-08): the `threads` feature and `slug_vm::Shared` landed
(geomean 1.01x tuned, 1.00x hold-out against `Rc`), and the inline caches
are relaxed atomics (0.99x). An audit asserting `Value` and `Program` are
`Send + Sync` found about 30 interior-mutable types in the way, through task
handles (a task's whole VM), channels, binding cells, global tables, and
native resources. Borrowing a `RefCell` writes its flag, so even concurrent
reads of one race.

**Decision (2026-11-08): partition, do not lock.** Making every type
thread-safe would put an atomic read-modify-write on every global and
binding read. Instead, what threads share is kept small and made safe to
read without locks: programs, `val` cells and module tables (written while a
module initializes, then read-only), and data values; channels, nursery
settlement, and the host registry are synchronized; everything else stays
with one thread, as phase 2's domain checks already guarantee. Scoped
`unsafe` is allowed at three sites, each in `slug-vm`'s `sync` module with
its invariant documented, and ThreadSanitizer runs in CI: the binding cell,
the module table (both `SoloCell`, which reads with no borrow count in
release builds and checks borrows in debug ones), and the move of a fork's
package to its worker (`unsafe impl Send`).

Open before phase 4: the entry program's own table still changes while its
top level runs, after a fork may have started. A later top-level
definition inserts an entry, and assigning one of its `var`s writes a plain
entry in place. Pre-declaring its top-level names as binding cells, as
modules' already are, would leave every table read-only once forks can run,
except for REPL redefinition, which needs its own rule.

Phase 3 outcome (2026-11-08): `Value` cannot be `Sync` by construction, since
its task handles and native resources are not, and the language already
refuses both at runtime as unsendable. A fork package's `Send` therefore
rests on that runtime check, and on everything the package shares with its
spawner being synchronized. The latter is not true yet, so the `unsafe impl
Send` lands in phase 4 with the worker that uses it, once these are
thread-safe:

- the host (`Rc<dyn VmHost>`), the module-variable registry, the progress
  driver, the nursery and its ready queue, the native resource registry,
  and the metrics;
- task state, waiters (`Rc<dyn Fn>` resumers), and select and root-waiter
  state;
- native callables and their library owners (`dyn Any`), which a
  `thread_safe` declaration vouches for.

Already thread-safe, and asserted at compile time under `threads`: binding
names (their kind is an atomic), strings (their offset table is a
`OnceLock`), binding cells and module tables (`SoloCell`), channel queues
(`Lock`), inline caches (`Hint`), a native function's thread-safe flag, and
a native module's registration sets. ThreadSanitizer is set up with the
first worker, when there are threads for it to watch.

### Phase 4: worker threads (desktop and server)

**Decision (2026-11-08): islands, one pooled thread per live fork.** A
fork's child VM moves to a worker thread with a scheduler of its own (root
nursery, progress driver, timers), so nursery, task, timer, and waiter state
never cross threads. It is linked to its spawner only through:

- a thread-safe completion slot for its result, which signals the spawner's
  `ProgressSignal` as a native producer does;
- channels, whose waiters become wakers: resuming a task on another island
  posts to that island's inbox and signals it, and the island runs the
  resume on its own thread;
- an atomic cancellation flag, read at calls and backward jumps;
- host calls (importing an initialized module, configuration, initialization
  queries), forwarded to the owning thread rather than requiring every
  `VmHost` to be thread-safe. Thread-safe natives run on the worker.

Each island has an empty native resource registry (a fork cannot use
resources) and its own metrics, merged when it settles; the fork domain
counter is a global atomic. The fork package's `unsafe impl Send` covers
moving an island, whose shared values the sendable check vets.

A blocked fork holds its thread and sleeps on its signal. The pool reuses
idle threads and grows when all are busy, so forks waiting on one another
cannot deadlock; the operating system spreads them across cores. One worker
per core with parked islands moved between workers stays a follow-up, taken
only if a measurement asks for it. The bullets below are the original
outline; the decision above refines them.

Progress (2026-11-08): islands run forks on pooled workers; wakes, results,
and host calls cross through per-thread inboxes; a program's threads detect
a deadlock together, so a blocked fork reports as a blocked task does. All
33 fork fixtures pass, 400 times over, eight runs at a time for half of
them. Open: host calls served only when the spawner waits, islands' metrics, and
scaling past four forks (below). ThreadSanitizer runs locally (nightly, `-Zsanitizer=thread`,
`-Zbuild-std`, CLI in `target/tsan`) and is clean on the fork fixtures; it
is not in CI yet. It found nothing that `fork/channel-relay` had not: a
`select` that checked and then parked as two steps, now one under its
channels' locks.

Experiment 2 rerun (2026-11-08) with real forks: 1000 players, 3 ticks,
each fork drawing every Nth player's page. The original drew with N worker
VMs that each initialized their own modules; here every fork shares
Crawl's modules. Crawl's read-only queries (`others`, `playerOf`, ...)
assign module `var`s through `actAs`, so forks need `acting`, `player`,
`players`, `dirty` (game), `gen` (rng), and `cameras` (web) as `task var`s
(done in a scratch copy only; Crawl itself is unchanged). Drawing time:

| forks | experiment 2 (own modules) | real fork |
| --- | --- | --- |
| 1 | 14.5 s | 14.7 s |
| 2 | 7.4 s | 8.7 s |
| 4 | 4.6 s | 5.9 s |
| 8 | 3.7 s | 7.8 s |

The rerun found the native adapter race and the copied `task var` updates
(both fixed). Scaling past four forks is limited by contended reference
counts: every call clones the frame's program, global table, and closure,
and under `threads` those are atomic counts on objects all forks share. A
microbenchmark of 4M calls to a shared module function takes 0.54 s on one
fork and 1.86 s on eight (14.3 s of CPU). Open: how to call shared code
without touching shared counts.

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
