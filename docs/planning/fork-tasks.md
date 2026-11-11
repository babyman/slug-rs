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
them. Open: scaling past four forks (below). An island counts and samples
on its own, and its counters (`VmMetrics::merge`: counts and times add,
peaks keep the larger) and call stack samples join its spawner's when it
settles, so `--metrics` and `--profile` see forks' work. A fork's
host calls are served at the next call, return, or `recur` of any VM on its
spawner's thread, not only when the spawner waits: each thread's inbox has
an attention flag, which those points read (one load) in place of the
cancellation check, raised by a queued host call or a cancelled fork. ThreadSanitizer runs locally (nightly, `-Zsanitizer=thread`,
`-Zbuild-std`, CLI in `target/tsan`) and is clean on the fork fixtures;
`scripts/tsan-forks.py` runs it, and CI's `thread-sanitizer` job runs that
on a pinned nightly. It found nothing that `fork/channel-relay` had not: a
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
fork and 1.86 s on eight (14.3 s of CPU).

Step 2a (2026-11-08): frames borrow their program and global table from
their closure, so a call counts one shared reference instead of three.
Paired runs of experiment 2 (same build settings, alternating binaries, on
a busier machine than the table above): one fork 17.3 s before and after;
eight forks 8.2 s to 7.0 s, scaling 2.1x to 2.5x. The callee closure and
shared data are still counted on every call and read. Kept in view, as the
user asked, whatever 2a measures: immortal code objects (programs,
closures, global tables, never counted once their module has initialized),
since per-call counting on shared objects worsens with every core added.
That needs a decision record before a custom reference-counted pointer.

The contention hunt (2026-11-08) found one more shared count per call: the
call path cloned the closure's program to look up its chunk. Borrowing it
instead makes calls to a fork's own closures scale like code without calls
(4M calls: 0.37 s on one fork, 0.08 s on eight). Experiment 2: 17.0 s on
one fork, 9.3 s on two, 5.6 s on four, 5.9 s on eight. What remains is
counting a callee every fork shares: 4M calls to a shared module function
take 0.59 s on one fork and 1.75 s on eight, and to a builtin native 0.36 s
and 0.91 s. That is the case immortal code objects (and natives) address.

Immortal code (accepted record `2026-11-08-immortal-code-objects.md`):
shared module functions now scale (4M calls: 0.55 s on one fork, 0.12 s on
eight). Experiment 2: 16.7 s, 8.7 s, 5.0 s, 4.4 s on 1, 2, 4, 8 forks, 3.8x,
against 3.9x for the original workers. Remaining: shared data is still
counted, notably constants a call clones from its program (4M calls to
`len("a")`: 0.36 s on one fork, 0.39 s on eight).

String constants made immortal with their program: `len("a")` 0.08 s on
eight forks; experiment 2 on eight forks 4.36 s to 4.12 s (paired), 4.1x one
fork. Still counted: the data `val`s hold. Making it immortal too (lists,
maps, and strings a module's `val`s reach) was tried and reverted: a `val`
map read on eight forks went from 0.48 s to 0.08 s, but experiment 2 gained
only 2% (Crawl's `val`s are nearly all functions), and it would extend the
memory immortality never frees from code to data. The compiler emits no
list or map constants, so there are none to mark.

One fork against the main thread (2026-11-08). One fork drew experiment 2
in 16.8 s, the original experiment's one worker in 14.5 s. Two findings:
builds without PGO differ by about 20% between commits on this workload
(PGO builds: 3ee46c4 8.30 s, f7c695a 8.39 s on the main thread), so
cross-commit timings need PGO; and a fork was 30% slower than the main
thread (PGO 10.9 s against 8.4 s). Arithmetic, allocation, and reading
shared data cost the same in a fork; calls did not. Every call to a closure
from another domain, which in a fork is every module function, took the
general call path to have its captures checked, though a closure with no
captures has nothing to check, and the general path's copied arguments left
strings shared, so `+` copied them. Only closures with captures take it now,
and task views hash variable addresses with one multiply rather than
SipHash. One fork: 14.5 s against 13.9 s on the main thread (profiling
build). Experiment 2: 7.4 s, 4.2 s, 3.7 s on 2, 4, 8 forks, as fast as the
original workers' 3.7 s.

`SoloCell` hardening. Debug builds with `threads` check its rule on every
access (`sync::solo_check`): each cell records which thread has reached it,
and a write from another thread while any fork runs panics at the write.
Unlike the borrow count and ThreadSanitizer, it does not need the read and
write to overlap in time. The workspace suite, experiment 2 (100 players,
eight forks) and the channel stress programs run clean under it; with the
fork write rule switched off, it fires at the write. A panic on an island
now becomes the fork's error instead of leaving its spawner waiting.

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

Tasks on `no_std` (2026-11-09). `platform::Instant` is `std`'s on hosted
builds and, on `target_os = "none"`, microseconds on a clock the host gives
each VM (`Vm::set_clock`, `ArtifactHost::set_clock`; the VM's tasks share it
through the progress driver). Timers take "now" from it. The blocking
adapter spins on a freestanding host until a timer is due or a native
producer sends; hosts that drive `poll` themselves ask `Vm::next_timer_in`
how long they may sleep. `slug-vm` keeps no dependencies. The ESP32-S3
firmware builds with `concurrency`, provides `chan` and `close`, reads the
system timer, and `esp32-s3-tasks.toml` runs three blinking tasks and a
counter on one core (CI checks it). Firmware text grows 55 KB (407 to 463
KB); static RAM is unchanged. On the board (2026-11-10) the tasks demo ran
three minutes, 920 blinks at the expected 5.08 a second, after a startup
bug from 735fa7f was fixed (builtins passed as module providers).

Parked (2026-11-10): M:N scheduling. Each island holds a thread for its
whole life, so forks beyond the core count that compute at once are
time-sliced by the operating system, and forks that wait hold a sleeping
thread each (capped at 256). A pool of about one worker per core that
resumes any ready island would bound threads by cores and avoid
oversubscription, at the cost of islands resuming on any thread, which
their per-thread inbox, wake routing, and single-threaded cells rule out
today.

What oversubscription costs was measured first
(`scripts/fork-oversubscription.py`): 128 million loop iterations split
evenly across the forks, a release build, an 8-core Apple machine (4
performance, 4 efficiency cores), medians of 5 interleaved rounds.

| Forks | `nursery limit` | Wall | vs 8 forks | CPU | Involuntary switches |
|---:|---:|---:|---:|---:|---:|
| 8 | none | 1.80 s | 1.00x | 12.0 s | 10.1k |
| 16 | none | 1.73 s | 0.96x | 12.2 s | 11.8k |
| 32 | none | 1.76 s | 0.98x | 12.4 s | 11.6k |
| 64 | none | 1.75 s | 0.97x | 12.4 s | 12.3k |
| 128 | none | 1.73 s | 0.96x | 12.5 s | 12.5k |
| 32 | 8 | 1.82 s | 1.01x | 12.4 s | 10.2k |
| 64 | 8 | 1.79 s | 1.00x | 12.3 s | 10.5k |
| 128 | 8 | 1.79 s | 1.00x | 12.3 s | 10.5k |
| 32 | 4 | 2.45 s | 1.36x | 9.7 s | 0.9k |

Up to 16 times as many forks as cores cost nothing measurable in wall time
(within the 11-19% spread), about 3% more CPU time, and about 20% more
involuntary switches: the operating system absorbs oversubscription, and
pacing with `nursery limit` neither helps nor hurts. Four at a time ran 36%
slower on 21% less CPU time, consistent with the performance cores alone;
eight forks scaled about 4.3 times over one core (about 7.7 s), as four
performance and four efficiency cores would. So M:N would not buy
throughput for computing forks; what it would still buy is fewer threads
and less memory for forks that mostly wait, which the cap bounds for now.

Untested, and reasons to come back: other operating systems or uniform
cores; forks each walking a large working set, where time-slicing could
thrash caches (the next variant to run, giving each fork a few MB of data);
and programs holding thousands of waiting forks, where threads' memory
would matter.

Decision (2026-11-10): the ESP32-S3 stays on one core for now, forks taking
turns, which passes the fork suite there; its programs so far wait on timers
and pins, so a second core would buy nothing measurable yet. The long-term
goal is the full port, forks as islands on the second core, so `fork` means
the same everywhere; one VM per core joined by channels was rejected as a
second, embedded-only model. The port needs atomic counts on Xtensa
(measure their cost on the board first), a `no_std` worker started on core 1
with a cross-core wake in place of the condvar, and fork limits sized to the
board's RAM. Toward it, an island is built from an explicit package of fork
inputs, free of `std` thread types, which a core-1 worker can reuse.

Review (2026-11-10) of the task and fork scheduler found that a busy fork
starved its siblings on cooperative builds, so `cancel-busy` and
`cancel-nested` hung there; no CI job ran the fork suite without `threads`.
Fixed: forks yield after 10,000 calls, returns, or `recur`s, while the
scheduler holds their domain so one fork's tasks never interleave; `spawn`
keeps its run-until-suspended semantics, which programs such as Crawl rely
on. Settlement now checks for a child's failure between turns. Also fixed:
an explicit nursery's body ran to completion inside `poll`, blocking on its
timers (a host-driven `poll` of `nursery { select { after 300 } }` took 304
ms). The starting task now suspends on the body as on an await, the body
suspends until its children settle, and every nursery shares the parent's
ready queue and timers; a child's failure is reported to its nursery rather
than found by a per-index scan. Fork admission had two dispatch paths, one of
which left a stale ready-queue entry; `Nursery::start` is now the only way a
task begins. Last, an island received a whole `Vm` and overwrote its
parent-owned fields; it is now built on its worker from `ForkInputs`, with
what stays the spawner's in a separate `ParentLink`, through the one
constructor tasks use (`Vm::closure_vm`, from `Inherited`).

### Phase 6: Crawl and evaluation

Draw Crawl's pages in forks on threads and measure the 1000-player server
against the experiment's numbers. Add an agent evaluation task that needs
parallel work, and triage what agents get wrong with `fork`, `task var`, and
the write errors.

Crawl draws in forks (2026-11-09). `web.push` gives every nth present
player to each of `forks` forks; each finds what its players' pages need
(`changesFor`, pure) and the ticker keeps cameras, maps and parts and sends
(`send`). The write errors found every global drawing wrote, one at a time:
`player`, `acting`, `companies` and `sights` became `task var`s; the
read-only queries moved from `actAs` to `lookAs`, which saves nothing back
(`actAs` rewrote `players` and marked the player dirty on every query); and
`rng.within`, which `game.land` names places with, swapped the game's
generator, so it now scopes a `task var` generator instead. `players`,
`dirty` and `gen` cannot be `task var`s here, unlike in experiment 2: the
ticker and the HTTP handlers are spawned tasks, which would each get their
own copy. Cost on one task: none (ticker in a spawned task 0.99x,
`crawl-ticks` 0.96x).

Drawing 1000 players' pages once (seed 6, 3 ticks played, client maps):

| | this task | 1 fork | 2 | 4 | 8 | 16-64 |
| --- | --- | --- | --- | --- | --- | --- |
| HEAD | 3.8-4.1 s | 4.1 s | 2.2 s | 1.25-1.6 s | 1.3-1.5 s | 1.3 s |
| `val` data immortal (f7c695a reapplied) | 3.7 s | 3.8 s | 2.1 s | 1.2 s | 1.15 s | 1.2 s |

Whole ticks with 1000 present players: 6.0 s on one task, 2.4 s on four or
more forks; the world alone (no one present) 0.3 s. Scaling stops at about
3x, at four forks; more, smaller shares do not help, so it is not the
efficiency cores' share. About 16% of the forks' samples are atomic count
updates on objects every fork shares, mostly not game data:

- module bindings: reading `game.playerOf` clones the export's
  `Value::Binding`, its `Arc<SoloCell>` and `Arc<BindingName>`, then
  resolves and drops it (about 40% of the atomic samples);
- native functions reached through module bindings: `NativeFunctionInner`
  counts (not immortal: the walk does not look inside binding cells) and
  their `ForeignDeclaration` and name `Arc`s on every call;
- data: the snapshot's `players` and `levels` maps, read by every fork, and
  the templates in `val`s (which f7c695a removes).

Fixed for the first two (2026-11-09): a module's binding cells and names,
declared natives' declarations, and its import maps (`val game =
import(...)`, a map of bindings) are immortal with its code, and a native
call borrows its member key. 1000 pages: 1.26 s to 0.98 s on eight forks,
0.92 s on 32 (4.1x one fork, from 3x); atomic updates fell from 16% of the
forks' samples to 5%, now nearly all the snapshot's data (`players`).
With f7c695a (`val` data immortal) reapplied on top, nothing changes
(eight forks 0.90/0.92 s against 0.88/0.92 s; one fork 1% slower): its
earlier gain here was the import maps, which no longer need the data walk.
It stays reverted.

Scaling work stops here (2026-11-09): what is left is near this machine's
ceiling (four forks 3.55x, eight 4.1x on 4P+4E), and removing every
remaining atomic update would gain at most 5-10%. Open: rerun the 1000-page
draw on a 16-32 core machine, where contention grows. If the atomic share
grows there, the general fix is a fused read of a module variable's entry
(`g[k]`, `g.field`) that does not clone the container, and on Crawl's side
one `lookAs` per page rather than one per query.

Agent evaluation (2026-11-09): project `colony` (eval/agent/projects/colony)
starts from a working one-core simulation whose module vars hold what
Crawl's drawing wrote. Codex, testing arm: a pilot (59 cases then) and three
repeats (71 cases), all passing every case in 89-125 s, 13 turns, 4-9 slug
runs, no failed run, so no write error ever fired; the agents read the
spec's fork section and designed around it first. Ratios by their own
benchmarks 0.31-0.45. Three of four reasoned about a `task var` cache going
stale when another task starts a new world (the pilot found the gap in the
suite and the reference's bug); one avoided it by passing each ant as a
parameter and keeping the cache a shared `var` only the ticker writes. The
main performance trap was where the cache lives: one repeat reached 0.58
before seeding its forks from the shared cache. The spec now gives the
version-number pattern for `task var` caches. The project saturates for
Codex as the earlier ones did.

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
