# Parallel tasks fork a snapshot of the program

## Status

Accepted. Not yet implemented.

## Amends

The "Scale-out by isolation" paragraph of
[Native interface profiles](2026-10-10-native-interface-profiles.md), which
adopted multiple isolated VMs exchanging owned values as the direction for
parallelism. That record points here.

It extends [cooperative task runtime](2026-08-23-cooperative-task-runtime.md)
and [shared mutable captures](2026-08-16-shared-mutable-captures.md) with
`fork` and `task var`, and leaves `spawn` and ordinary `var` unchanged.

## Context

A Slug program runs on one host thread. Tasks are cooperative and share live
state by design: a spawned task snapshots its immediate lexical bindings, but
outer captures and module globals stay live. Programs rely on both halves of
that contract: on live sharing, and on no other task running between a task's
suspension points. In the repository's own code:

- Crawl's tick loop (`web.slug`) runs while the module global `serving` is
  true; the spawner sets it to false when `http.serve` returns, which stops
  the loop. The tick changes the world in module globals that request
  handlers in other tasks read.
- HTTP's connection tasks call a handler that reads and replaces Crawl's
  `streams` map without locks, which is safe only cooperatively.
- HTTP's header timer is wired through the module globals `expired` and
  `cancelled`.

Ordinary `spawn` therefore cannot be made parallel, by a build flag or
otherwise, without changing what these programs mean.

Modern hosts are multi-core, including the ESP32-S3 firmware target. The VM
must still run on a single thread, but it must not be limited to one.
Measured on a 1000-player Crawl server, drawing pages is about 90% of a tick
and needs only read access to the world.

Slug values are immutable to programs: strings, bytes, lists, maps, structs,
and enums change only through new values, and the VM changes one in place
only when nothing else holds it. Mutable state lives only in binding cells,
module globals, channels, tasks, and native resources. Sharing data between
threads is therefore safe in the language. The obstacle is the
implementation, non-atomic `Rc` counts.

Experiments on `experiment/arc-values` (see its `experiments/parallel/`)
measured the options:

- Atomic counts on data values only cost a geomean of 1.03x on tuned
  benchmarks and 1.02x on hold-outs. Data-heavy workloads cost 1.04-1.10x;
  arithmetic is unaffected.
- One shared 1000-player Crawl snapshot, drawn by worker VMs on 1, 2, 4,
  and 8 threads, took 14.5, 7.4, 4.6, and 3.7 s. Workers with private deep
  copies were no faster, so the shared counts caused no measurable
  contention. Separate processes sharing nothing scaled the same, so the
  ceiling was the host (4 performance and 4 efficiency cores), not sharing.
- A deep copy of the whole snapshot took about 4.5 ms.

## Decision

### `fork` starts a task that sees a snapshot

`fork { ... }` starts a task like `spawn` does: it belongs to the current
nursery, yields a task handle, and is awaited, selected on, cancelled, and
failed under the existing nursery rules. What differs is what it sees.

- **Everything is a snapshot.** At the fork, the task takes a snapshot of its
  immediate and outer captures and of every module's globals, as they are at
  that moment. Later changes made by the spawner are not visible to it.
- **Results cross by value.** The only way out is the task's result, and
  later channels (below). A result must be sendable.
- **Writes are checked.** Changing shared state inside a fork is an error,
  except for `task var`s (below).

These are the semantics of a process `fork`, at task granularity. Because
data values are immutable, the snapshot costs one reference-count increment
per binding, not a copy.

`fork` is a reserved word.

### Each keeps its meaning on every host

| | Multi-threaded host | Single-threaded host |
|---|---|---|
| `spawn` | cooperative on the spawner's thread, sharing live state | the same |
| `fork` | may run on a worker thread, sees a snapshot | runs cooperatively, still sees a snapshot |

A host or build without threads runs forks cooperatively with the same
semantics, so a program's results do not depend on how many threads the host
has, and the single-threaded VM remains the full language. The build chooses
how forks run, never what they mean.

### `fork` is the language; `threads` is the default build

`fork` is part of the language wherever tasks are. Every build with the
`concurrency` capability has `fork` alongside `spawn`; a build without it
has neither, and both are a capability error there, as `spawn` is today. No
program asks whether threads exist: a build with tasks but without threads
simply runs forks one after another.

The ESP32-S3 firmware builds without `concurrency` today, because the
scheduler and timers depend on `std`. Bringing tasks to `no_std` targets is
part of implementing this record, so that multi-core embedded targets get
both `spawn` and `fork`.

The `threads` build capability, which gives forks worker threads, is the
default on every target with more than one core: desktop and server hosts,
and multi-core embedded targets such as the ESP32-S3. Only single-core
targets build without it, and they often lack the atomic instructions it
needs; the single-core RISC-V ESP32-C3 has none. On an embedded host without
OS threads, the firmware runs a worker per core and drives each with
`Vm::poll`; on ESP-IDF, a FreeRTOS task pinned to each core can do it.

`threads` costs single-task programs the price of atomic counts on data
values: measured at a geomean of 1.03x, and up to 1.10x on data-heavy
workloads. It ships at that cost. Reducing reference-count traffic, such as
borrowing instead of cloning on reads and moving on a value's last use,
helps both builds and proceeds as ordinary VM optimization. Biased reference
counting, where the owning thread counts without atomics, is the larger
option if the cost needs to fall further.

### `task var` is a variable each task owns

```slug
task var player = {}
task var acting:str|nil = nil
```

A module-level `var` declared with `task` belongs to each task. Every task,
whether started by `spawn` or `fork`, starts with its spawner's current
value; its writes are its own and are discarded when it settles. The main
task starts with the declared value. This is the model of Swift's
`@TaskLocal`, C#'s `AsyncLocal<T>`, and Python's `ContextVar`.

`task` is contextual: it is a declaration modifier only immediately before
`var` at module level, so `task` remains usable as a name. `task val` and
`task var` inside a function body are compile errors.

### What a fork may write

Inside a fork, including in tasks it spawns and in any function it calls:

- assigning a captured function-local `var` is an error;
- assigning a module `var` is an error unless it is a `task var`;
- assigning its own locals and `task var`s is allowed.

The type checker reports assignments it can see in the fork's body at the
fork site. Assignments reached through calls are checked when they run: the
error names the variable, the write, and the fork, and says to return the
value from the fork or to declare the variable `task var` if each task's
copy is its own.

Errors were chosen over silently private copies by what Crawl's drawing
writes:

| Kind | Crawl globals | Purpose |
|---|---|---|
| Scratch context | `player`, `acting`, set and restored by `actAs` | stand-in for passing context down the calls |
| Memo caches | `sights`, `companies` | speed only |
| State that must persist | `shownParts`, `shownMaps`, `cameras` | what the next tick compares against |

Private copies would run the first two rows correctly and lose the third
silently: every part resent each tick, the camera jumping, and nothing
failing. That is the worst failure for an agent, which learns from errors
and may not notice. Errors alone would reject the first two rows, which are
correct, and would make a cache inside a library break its fork callers.
`task var` lets the variable's author, the only one who knows whether a
write should persist, mark the first two kinds once; anything else fails
loudly and points at the redesign, returning the updates for the world task
to apply.

Starting strict is also the reversible choice: allowing a write that was an
error breaks no program; making a silent write an error would.

A memo cache in a `task var` has one hazard (found by the first agent run of
the `colony` evaluation, 2026-11-09): no task can clear another task's copy.
When what it caches can be replaced, as a new world replaces the old, the
copy in a task that did not replace it goes stale. The language
specification gives the pattern: a shared `var` counts replacements, and the
cache records the count it was filled at. Crawl's `sights` is safe today
only because its world is replaced before the server's ticker is spawned. A
`task var` every task resets together would remove the pattern; it waits for
a second case.

### A fork's snapshot copies the `var`s

Every top-level name of a module is a binding cell, and an importer holds
the exporter's cells. A `val`'s cell is written once, when its module
initializes, and never changes after; a `var`'s cell changes whenever the
program assigns it. A fork therefore shares every `val` cell, which covers
nearly all global reads, functions among them, and takes a private cell for
each `var`, holding the `var`'s value at the fork. Reads of a `val` in a
fork go through the shared tables exactly as outside one.

This is the mechanism `task var` needs for every task: a fork treats every
`var` as per-task, and the write rule refuses the writes to those that are
not `task var`s. One per-task cell mechanism serves both.

The cost grows with the number of `var`s, not the size of the program.
Measured on Crawl's server (`experiments/parallel/census.txt`), 25 modules
hold 1365 global entries, 957 of them binding cells, and 28 `var`s. A
snapshot of the `var`s takes at most about 1 µs, against about 14.5 ms to
draw one player's page.

Two cases follow from `val` cells being written at initialization:

- **`fork` during module initialization is an error.** A fork started from a
  module's top-level code could see that module's `val` cells filled after
  it started.
- **REPL redefinition is live.** The REPL redefines a name by writing its
  existing cell, so a running fork sees the new value. The person at the
  REPL is the spawner, and a redefinition is a deliberate change to the
  program, not a write by it.

### Native functions and imports in a fork

- **Native functions declare thread safety.** A native library is not
  necessarily safe to call from several threads. A clutch manifest declares
  which of its native functions are, and calling an undeclared one in a fork
  is an error on every build, so a program's behavior does not depend on
  whether the build has threads. A function entry declares it with
  `thread_safe = true`; it does not change the generated shim. The core
  clutch's pure functions are declared, and its stdin reader and signal
  registration, which start threads and change process state, are not.
  Running undeclared calls on the owner thread instead is possible later
  without changing any program that works.
- **Imports read; initialization writes.** Importing a module that is
  already initialized is allowed in a fork. Importing one that is not yet
  initialized is an error, because initializing it runs top-level code that
  writes its globals.

### Sendable values

A value is sendable when it is nil, a boolean, a number, a string, bytes, an
enum case, a struct schema, a channel, or a list, map, or struct whose
contents are sendable. A closure is sendable when every value it captures is
sendable and it captures no `var`; its code is immutable. A captured `var`
is a binding cell its creator can still change, so a closure over one, such
as `val inc = fn() { n = n + 1 }`, is not sendable: a fork that calls it
fails, whether it would read or write `n`. Module functions read module
globals through the per-task cells above, not through captures, so they
are sendable. Task handles, binding cells, and native resources are not
sendable in the first version.

### Channels cross forks

A fork can send on and receive from any channel it holds, and a value sent
on a channel must be sendable. A task parked on a channel is woken by a send
or close from any thread. `select` works across forks the same way: a
fork's `select` over channels shared with its spawner and its `after`
timers behave as in a single task. Forks therefore stream results as they
go, and a pool of forks can pull work from one channel.

Snapshotting an unsendable value is not an error: the fork can hold it but
fails, with a checked runtime error, if it uses it. The type checker reports
an unsendable capture or result at the fork site where it can prove one.

## Consequences

- **`task var` under `spawn`.** A `spawn` task's writes to a `task var` no
  longer reach its spawner. No existing program changes, because no existing
  variable is a `task var`. Marking one is a decision about that variable,
  made by its module's author.
- **Value representation.** Builds with `threads` count data values,
  closures, and channels atomically; binding cells keep non-atomic counts,
  since a fork never shares one it can write. Builds without `threads` keep
  `Rc` throughout.
- **Channels and the scheduler.** In `threads` builds a channel's state sits
  behind a lock, and parking and waking go through a scheduler that every
  worker thread can reach: a send on one thread makes a task parked on
  another runnable. Builds without `threads` keep today's single-threaded
  channels. Channel operations, not data values, are where the threaded
  build pays for locking, and they need their own benchmark.
- **Struct and enum identity.** A fork shares its spawner's module instances
  and their schemas, so struct identity, enum cases, and type matches agree
  across tasks. Isolated module instances would re-create schemas and break
  this, which is one reason isolates are not the primary model.
- **Globals per task.** Every task resolves `task var`s through its own
  cells, and a fork resolves every `var` through its own cells; closures it
  calls, including exported functions, resolve the same way. `val` reads are
  unchanged. A `var` read or write checks whether the running task has its
  own cell for it, and a write in a fork whether the binding is a
  `task var`; outside forks and `task var`s the check is a flag test. Its
  cost on `var` reads in a fork is measured when `fork` is built.
- **Shared bytecode.** Programs are shared across threads, so per-instruction
  inline caches, such as map-key hints, must be thread-safe or kept per
  thread.
- **Spawner copy-on-write.** After a fork, a spawner that changes a value the
  fork still holds copies it, as it would for any shared value. Large flat
  maps and lists pay a full copy on their first change.
- **Scheduling.** Hosts that offer threads run forks on a pool of worker
  threads, one task per thread at a time. Under host-driven progress
  (`Vm::poll`), an embedded host can poll a worker on each core.
  Cancellation stays cooperative: a fork observes it at calls and backward
  jumps.
- **Determinism.** Forks cannot affect each other or their spawner except
  through results, so a program that awaits them in a fixed order is as
  deterministic as its sequential form. Crawl's replayable ticks are
  unaffected.
- **Crawl.** To draw in forks, Crawl marks `player`, `acting`, `sights`, and
  `companies` as `task var`s, and its drawing returns the `shownParts`,
  `shownMaps`, and `cameras` updates for the world task to apply. Its forks
  can send finished pages to the stream channels they find in `streams`
  directly; only changing `streams` itself stays with the world task.
- **Not provided.** Shared mutable state between forks, locks, and atomics
  are not provided. Mutable state is shared by owning it in one task and
  communicating.
- **Built on it, later.** A data-parallel `map` over a list can be a library
  function over forks. Whether it, rather than `fork`, is the primary
  surface for data parallelism is deferred until `fork` has been used in
  real programs such as Crawl.

### Alternatives considered

- **Parallel ordinary `spawn`.** Rejected: live outer captures and module
  globals make it a data race, and the programs in Context depend on them.
- **Choosing `spawn`'s semantics with a build flag.** Rejected: one program
  would mean different things on different builds, and a library could not
  know which it runs under.
- **Inferring per `spawn` site that sharing and snapshot semantics agree,
  and running those sites in parallel.** Rejected: none of the repository's
  spawn sites qualify, since they exist to coordinate; Crawl's parallel work
  is a loop, not a spawn; and a small edit, such as a counter, would silently
  make a site serial again.
- **Private copies of every global in a fork.** Rejected: state that must
  persist is lost silently (see the table above).
- **A checked error for every write.** Rejected: it rejects correct scratch
  and cache writes, and makes a library's private cache part of its
  interface.
- **The name `local`.** Rejected: in Lua, Bash, and R it means lexically
  scoped, and in Perl a dynamic rebinding. `thread_local` was rejected
  because a cooperative fork is not a thread; `private` reads as the
  opposite of `export`.
- **Isolates with fresh module instances.** The previous direction. They
  re-run module initialization, re-create schemas, and need state passed in
  explicitly, as Crawl's experiment worker did with `game.load`. Fork gives
  the same isolation of writes without these costs. Isolates remain possible
  for a long-lived worker that should not see the spawner's state.
- **Copying every global table at the fork.** About 15 µs for Crawl, which
  is affordable, but it grows with the program's size and sends every
  global read in a fork, including every function call, through the fork's
  own tables.
- **Sharing each global table copy-on-write.** About 0.06 µs at the fork,
  but the spawner's first write to a module afterwards copies that module's
  table (up to 3.3 µs for Crawl's `game`), and reads in a fork still go
  through its own tables. A lazy snapshot, copying a module's table when the
  fork first uses it, needs this sharing to see the fork-time version, so it
  is the same option.
- **Deep-copying values instead of sharing them.** It avoids the
  atomic-count cost, and it was cheap for Crawl (4.5 ms). But it grows with
  the program's whole state at every fork.
- **Shared memory with locks.** Rejected: it conflicts with value semantics,
  costs every program, and does not fit `no_std` hosts.

## Open questions

1. The primary surface for data parallelism (see "Built on it, later").

## Migration

None. `spawn` and ordinary `var` keep their meaning. `fork` becomes a
reserved word; no code in the repository uses it as a name.
