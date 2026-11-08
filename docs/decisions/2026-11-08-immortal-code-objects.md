# Code shared by forks is never counted

## Status

Proposed.

## Extends

[Parallel tasks fork a snapshot of the program](2026-11-07-parallel-fork-tasks.md),
whose forks share every module's code with the program that made them, and
the `threads` build's atomic reference counts (`slug_vm::Shared`, phase 3 of
`docs/planning/fork-tasks.md`).

## Context

Under `threads`, every value the VM shares is counted with `Arc`: cloning or
dropping one is an atomic read-modify-write on its count. When forks on
several cores clone and drop the same object, its cache line moves between
the cores on every operation, and those operations no longer run in
parallel.

A call clones its callee and drops it on return. After the program and
global table stopped being counted per call (`ae819c8`, `23ac2f2`), a call
to a closure only its own fork can reach scales as code without calls does.
A call to a closure every fork shares does not:

| 4M calls to | 1 fork | 8 forks |
| --- | --- | --- |
| a closure the fork made | 0.37 s | 0.08 s |
| a shared module function | 0.59 s | 1.75 s |
| a builtin native (`len`) | 0.36 s | 0.91 s |

Real programs call shared code constantly. Crawl's pages, drawn by forks
from one world (experiment 2, 1000 players, `docs/planning/fork-tasks.md`),
take 17.0 s on one fork, 9.3 s on two, 5.6 s on four, and 5.9 s on eight;
workers that each initialized their own modules took 3.7 s on eight. The
gap grows with every core added, which matters more on larger machines than
on this 4P+4E laptop.

The objects every fork calls through are a module's code: its programs, the
closures its top-level declarations bind, its global tables, and the native
functions its clutches install. A fork cannot start while a module
initializes, and after initialization these objects stay alive until the
program ends: a module's table holds them, and modules are not unloaded.
Counting them achieves nothing; it only costs.

## Decision

1. **Immortal code.** Under `threads`, a module's code becomes immortal when
   the module finishes initializing: its programs, the closures and native
   functions bound in its global table, and the table itself. Cloning or
   dropping an immortal object reads its count and does nothing else. Every
   core can keep the line in its cache, as it does any data no one writes.
2. **Code only, not data.** Values a module's `val`s hold (strings, maps,
   lists) stay counted. Marking them would mean walking them at
   initialization, and the measurements above do not yet show them limiting
   scaling. A later record may extend immortality to `val` data if a
   measurement asks for it.
3. **One pointer type.** `slug_vm::sync` gains a reference-counted pointer
   for the four immortal-capable types (`Program`, `Closure`, the global
   table, and a native function's inner state), with a count whose top bit
   marks immortality. It replaces `Arc` for those types only; every other
   value keeps `Arc`, and builds without `threads` keep `Rc` everywhere.
   Its `unsafe` (allocation, dereference, release) stays in that one module,
   each block with its invariant, checked by Miri and by ThreadSanitizer on
   the fork tests.
4. **What is not made immortal.** Closures made at run time (a function
   literal inside a function, a task body) stay counted: they are usually
   reached by one task. So do channels, tasks, and resources, whose release
   does work.

## Consequences

- Calls to shared module functions and natives scale across cores as calls
  to a fork's own closures do. The target is experiment 2 within 10% of the
  workers that initialized their own modules (3.7 s on eight).
- Every clone and drop of a code object gains a load and a branch before its
  atomic operation, and loses the atomic operation when the object is
  immortal. Data values are unchanged. A single-threaded program should see
  no measurable cost; a PGO A/B with hold-outs decides.
- Immortal code is never freed. Modules already live for the whole program,
  so this costs nothing there. Redefining a name in the REPL, or a host that
  reloads modules, leaks the replaced code: bounded by how much is
  redefined, and accepted for now.
- A native function's library is never unloaded once its module has
  initialized. Libraries that hand producers to foreign workers are already
  kept loaded, so this changes little.
- `Arc::get_mut` and `Arc::ptr_eq` semantics are kept by the new pointer:
  an immortal object always reads as shared, so nothing mutates it in
  place.
- More `unsafe` in `slug-vm`, which until phase 3 had none. It is confined
  to one module and one type.

## Alternatives considered

- **Per-island module instances.** Each fork gets its own copies of the
  module closures, as the experiment's workers had. It removes the
  contention without `unsafe`, but costs a copy per fork, gives a fork's
  functions a different identity from its spawner's, and leaves the data
  they reach shared.
- **Biased reference counting.** Each object's owning thread counts without
  atomics and other threads count atomically, merging later. It handles
  data as well as code, but changes every object's layout and every
  clone and drop, a much larger and riskier change than code alone needs.
- **Not counting a frame's callee.** A frame could borrow its closure while
  something else keeps it alive, such as its module table. That is unsound
  when the REPL redefines the binding while a call is running, unless the
  table keeps replaced values alive, which is immortality by another name.
- **Leave it.** Forks would scale to about three times one core on this
  machine and no further.

## Open questions

- Whether a runtime closure stored in a module `val` after initialization
  (a memo table of functions, say) should become immortal too.
- Whether the REPL should free replaced code when no task can still reach
  it, rather than leak it.
- Whether embedded `threads` builds (phase 5, dual-core ESP32-S3) need this
  or fit their two cores without it.
