# A native-code tier

## Purpose

Decide whether, and how, Slug compiles hot functions to machine code, to
take it past CPython and toward the JVM on the programs where the
interpreter is the limit. This is a scoping plan: it records why the
interpreter alone will not get there, a measured bound on what native code
could reach, and the decisions and phases a native tier would need. Nothing
here is built yet; Phase 1 is planned below.

## Why the interpreter will not close the gap

Against CPython 3.15 the interpreter is about 2.3x slower by geometric mean
(see "Where the benchmarks stand (2026-11-14)" in
[vm-optimization.md](vm-optimization.md)). The 2026-11-17 ceiling round in
that document priced every interpreter-level lever on PGO builds with
interleaved A/B runs:

| lever | what removing it buys |
|---|---|
| bytecode checks the verifier already proves | nothing (1.00x) |
| quickening and inline caches | about nothing: sites are 99% monomorphic, but the guarded operations are already cheap |
| reference-count traffic | 6-11% of a real program, part of it removable |
| allocator calls | 4-8% |
| dispatches a register VM removes | about 7% |
| the call path (23 ns a call) | at most about 7 ns of it: frame size about 3, the locals vector 2-4, loop re-entry and module-closure counts nothing |

Each lever is single-digit percent of a real program; together, over
several projects, perhaps 15-25%. The cost of an instruction is spread over
value copies, tag checks, stack traffic, and bookkeeping, none of which is
large alone. A 2x step needs code that keeps values in machine registers
between operations.

## Prototype bound (2026-11-17)

Two Slug functions were lowered by hand through Cranelift 0.113 (a scratch
crate, not committed) in two styles, and timed against the PGO interpreter
and Java 17 (Zulu 17.0.6) on the same Apple M3:

- **boxed**: a baseline compiler that keeps the VM's data model. Values are
  the 24-byte tagged `Value` layout in a memory frame; every operand gets a
  tag guard; every copy and overwrite checks for a counted (heap) value
  where reference counting would happen; a call checks the callee's
  identity in the global, pushes a shadow frame for error reports, and
  checks the attention flag. It assumes nothing about types.
- **typed**: values unboxed in registers, with Slug's semantics kept:
  checked `i64` arithmetic, a finite check after every float operation,
  and the attention check on each call or loop iteration. This is the
  ceiling for code whose types are known or guarded.

`fib(32)` is 7.05M calls; `advance` is n-body's loop
(`recur(remaining - 1, position + velocity, velocity + 0.000_001)`) run
20M times. Native times are the best of 5 in-process runs; Slug's are the
median of 7 processes less startup; Java's are the best of 15 after
warm-up, with `Math.addExact`/`subtractExact` and `Double.isFinite` checks
so it does the same work.

| | `fib(32)` | `advance` 20M |
|---|---:|---:|
| Slug interpreter (PGO) | 242 ms | 585 ms |
| CPython 3.15, derived (see below) | ~112 ms | ~380 ms |
| boxed native | 24 ms | 109 ms |
| typed native | 8.2 ms | 23.4 ms |
| Java 17 | 6.8 ms | 17.2 ms |

The CPython row is derived from the recorded per-call and per-iteration
times (15.9 ns a `fib` call, 19 ns an n-body iteration, 2026-11-14 and
2026-10-25), not run this time.

What the bound says:

- A baseline compiler that changes nothing about values is already 5-10x
  the interpreter on this kind of code, and 3.5-4.7x CPython.
- With types known, native code is within 1.2-1.4x of Java on the same
  checked semantics. Slug's checks (overflow, finiteness, attention) are
  not what separates it from the JVM.
- The four functions compiled in 5.5 ms together, including setting up
  the JIT; compile latency needs a tiering threshold but is not a blocker.

What it does not say:

- These are call- and arithmetic-bound kernels, the best case. Real
  programs such as Crawl spend much of their time in map lookups, string
  work, allocation, and reference counting, which run in the runtime's
  Rust code whichever tier calls them. Native code removes the dispatch
  and stack traffic around those calls, not the calls. **The gain on a
  data-heavy program is not measured yet** and is the first gate below.
- The lowering was written by hand for two functions. A compiler has to
  produce it from bytecode and handle everything the kernels did not touch
  (below).

## Gate 0: a data-heavy function (2026-11-17)

The sight cast is Crawl's hottest code: in `crawl-ticks`, `outdoorSight`'s
`cast` loop and the `around` closure it calls take 44% of the instructions
(19% and 25%), by the counting build's sampled stacks. Gate 0 compiled
that code, `rays` → `cast` → `at`, with a small baseline compiler in a
scratch branch rather than by hand: it reads the closures' real bytecode
and lowers each instruction on its own, in the boxed style above. Values
stay the VM's 24-byte `Value`s in a native frame. Integer arithmetic and
comparisons run inline behind tag guards, and so do truthiness tests and
copies of uncounted values. Everything else is a call into the VM's own
Rust code: index reads (with the chunk's map hints), map construction and
`UpdateCapture`'s merge, the native `len` call, the clone or drop of every
counted value, and every slow path, including module bindings read through
`level.WIDTH`. Calls between the three guard the callee's tag and chunk,
push a shadow frame, and check the attention flag, as the earlier `boxed`
kernels do.

A driver runs one player's sight 4,000 times on the world `crawl-ticks`
builds (seed 6, 40 ticks; 112 rays, 418 cast steps, 120 squares seen), on
the same closures and captures either way: interpreted, or through a test
builtin that enters the compiled `rays`. Both give the same squares and
checksum. Timings are medians of 9 interleaved rounds, less start-up:

| build | interpreter | boxed native | gain |
|---|---:|---:|---:|
| release | 381 µs a pass | 144 µs | 2.65x |
| PGO | 225 µs a pass | 136 µs | **1.65x** |

PGO speeds the interpreter 1.7x here and the compiled path hardly at all,
because the compiled code's time is almost all in the runtime calls. A
profile of the PGO compiled run:

| work | share |
|---|---:|
| index reads (`index_value`, hinted map reads, key comparisons) | 27% |
| clones and drops of counted values | 23% |
| map literal and the merge into `found` | 13% |
| reading module bindings through (`level.WIDTH`, `level.HEIGHT`) | 9% |
| the generated code itself | 8% |
| arithmetic slow paths taken because an operand was a binding | 8% |
| allocator | 6% |
| the `len` native call | 4% |

What it says:

- **The gate passes, narrowly: 1.65x against the 1.5x bar.** The gain
  is the dispatch and stack traffic around the runtime's calls, which is
  what the interpreter spends 40% of this code's time on.
- If time follows instructions, compiling only these two functions would
  make `crawl-ticks` about 1.2x faster. Compiling all of a data-heavy program
  at this ratio would make it about 1.6x faster, not the 5-10x of the
  numeric kernels.
- Type specialization, the second tier, has little to remove here: 92%
  of the compiled time is runtime work. More gain would have to come from
  the runtime operations: inline reference counts and list indexing in
  compiled code (most of the clone/drop share is a call into
  `Value::clone`'s or `drop`'s general match), and the cheap fixes the
  interpreter also wants.
- Two of those fixes are visible here and help both tiers. Resolving module bindings when they are read
  would remove the binding reads and the slow arithmetic, about 17% of
  the compiled time and a similar absolute cost in the interpreter. A
  table of one-character strings would remove an allocation and free per
  `row[x]`.
- The bound is somewhat generous to native code: the shadow frame is
  lighter than a VM `Frame`, and the fork-snapshot check on
  `UpdateCapture` was left out. Both are small against 325 ns a step.

## Constraints

- **The interpreter stays the reference and the only tier on embedded
  targets.** `slug-vm` builds `no_std` for the ESP32-S3; Cranelift does not
  target Xtensa and the device has no room for it. The native tier is an
  optional desktop crate behind a feature, and every program must run
  identically without it.
- **Annotations are not runtime facts.** The type checker reports only
  contradictions it can prove and annotations add no runtime validation
  (`type-system-plan.md`), so native code cannot unbox on the strength of
  an annotation alone. It specializes on observed types with guards, and a
  failed guard leaves the native code.
- **Semantics to keep:** checked integer overflow and finite floats as
  runtime errors with source spans; `defer`/`onerror` cleanup; `recur`;
  `match` patterns; closures and captured cells; module bindings; native
  calls through the existing ABI; suspension, `fork` attention, and the
  scheduler's turn-taking; immortal code objects under `threads`.
- **`unsafe_code = "deny"`** is workspace-wide. Entering generated code is
  unsafe by nature, so the native crate needs an explicit, contained
  exemption.
- **Cost:** Cranelift adds about 2.5 MB to a release binary and about 55
  crates to the lock file; a scratch binary with the four functions was
  2.7 MB against `slug`'s 2.7 MB today.

## Proposed shape

The crate boundary, the `unsafe` exemption, and the IR are fixed by
[a desktop native tier in its own crate](../decisions/2026-11-18-desktop-native-tier-crate.md).

1. **Baseline tier.** Compile a function's bytecode one instruction at a
   time into Cranelift IR over the existing `Value` layout, as the boxed
   prototype does: a guard and a fast path per instruction, and a call
   into the VM's existing Rust helper for every slow path and every
   complex operation (maps, strings, calls to closures that are not yet
   compiled, cleanup). The frame stays the VM's frame, so errors, spans,
   and unwinding keep working, and a function can enter or leave native
   code at any call. Functions with `defer`, `onerror`, `select`, or
   `fork` stay interpreted at first.
2. **Specialization.** Use the per-site type observations (the site counts
   showed 98.6-100% of arithmetic matching its site's most common type) to keep
   `Int`/`Float` locals and temporaries in registers between guards, and
   leave to the interpreter at the current instruction when a guard fails
   (deoptimization: write the registers back into the frame's slots and
   continue interpreting).
3. **Calls between compiled functions** pass unboxed arguments when both
   sides are specialized, with the boxed entry kept for every other caller.

Hot functions are found with a call and back-edge counter per chunk; a
function compiles after it crosses a threshold, on the VM thread at first.

## Phases and gates

Each phase is accepted on the usual interleaved PGO A/B with hold-outs, and
a phase that does not pay stops the plan.

- [x] **Gate 0: bound a data-heavy program.** Lower one of Crawl's hot
  functions (the sight cast's `around`, or `seeOutdoors`'s inner loop) by
  hand in the boxed style, calling the real runtime helpers for maps and
  lists, and time it against the interpreter. If the boxed tier gains less
  than about 1.5x there, the tier is worth it only for numeric code, and
  the decision below changes. *Done: 1.65x on PGO builds, see "Gate 0"
  above; passes narrowly, and nearly all the rest is runtime work.*
- [x] **Decide:** the exemption from `unsafe_code`, the Cranelift
  dependency, and desktop-only scope (a decision record). *Done: [a desktop
  native tier in its own crate](../decisions/2026-11-18-desktop-native-tier-crate.md),
  which also places the compiler's input in an IR lifted from verified
  bytecode.*
- [ ] **Phase 1: baseline tier** for straight-line arithmetic, locals,
  branches, `recur`, and calls to module functions; everything else stays
  interpreted. Measure `fib`, `function-call`, n-body, spectral-norm, and
  `invaders-game`. *Planned: see "Phase 1 plan" below.*
- [ ] **Phase 2: the rest of the instruction set** through runtime helpers:
  index reads, collection construction, closures and captures, `match`,
  native calls. Measure the Crawl workloads and `http-server`.
- [ ] **Phase 3: type specialization with deoptimization.**
- [ ] **Phase 4:** cleanup (`defer`/`onerror`), tasks and forks in native
  frames, compiling off the VM thread.

## Phase 1 plan (2026-11-18)

The baseline tier, within the boundaries the
[decision record](../decisions/2026-11-18-desktop-native-tier-crate.md)
sets. Four facts from the VM shape it:

- Every Phase 1 kernel loops through `recur`, which restarts the frame at
  instruction 0. One entry point, at instruction 0, therefore covers both
  calls and loops; a loop that becomes hot is entered at its next `recur`,
  with no separate way to enter a running frame.
- The compiler emits `EnterScope` and `LeaveScope` only for blocks that
  contain `defer`. Ordinary branches are plain jumps.
- A chunk without `MakeClosure` captures none of its locals, so its locals
  can live in the native frame without touching the VM's `LocalSlot`
  layout.
- The tier exists only in the `slug` binary, which builds with `threads`.
  Native code checks the attention flag synchronously; the cooperative
  builds' yield at `recur` never arises.

### Execution

- **Entry.** The dispatch loop already looks up the active chunk again
  whenever `frame_epoch` changes. There, if the frame is at instruction 0
  and its chunk has a compiled entry, the VM calls the entry instead of
  interpreting: one load and a branch, on calls and loops only. On return
  the VM pops the frame and pushes the value, as `Return` does.
- **Native activations.** A compiled function keeps its locals and
  temporaries as 24-byte `Value`s on the machine stack, and pushes a small
  record onto a list the VM owns: the closure, the call span, and the
  current instruction (Gate 0's shadow frame). A call between compiled
  functions builds no VM `Frame`.
- **Leaving native code.** At a call native code does not handle, or when
  the native records reach a depth limit, the innermost activation calls an
  exit helper. It walks the records from the bottom and builds a VM `Frame`
  for each, from the frame snapshot at that activation's current
  instruction; every activation then returns "exited" and the interpreter
  continues from the innermost frame. The exit is all or nothing: a native
  frame never waits on the interpreter, so a task can still suspend
  anywhere. Phase 3 leaves at a failed guard by the same mechanism.
- **Errors.** A helper that can fail returns a status. The error path turns
  the records into frames, so stack traces come from the same data as
  today, and overflow and non-finite errors keep their spans.
- **Depth.** The interpreter has no recursion limit, so native code adds
  none: past its stack budget (256 KiB of machine stack from the entry,
  in bytes because frames differ in size), a call leaves native code and
  the recursion continues in interpreter frames.

### What compiles

A chunk compiles only if every instruction in it is one of these;
otherwise the whole chunk stays interpreted:

- `Constant` (values, not functions), `Nil`, `True`, `False`, `Pop`,
  `Duplicate`;
- `GetLocal` (including moves) and `SetLocal`;
- `GetGlobal`;
- arithmetic, comparisons, `Not`, `Negate`, `LocalConstantBinary`, and
  `ConstantBinary`;
- the jumps and `JumpIfProvided`;
- `CallPositional`, `CallGlobal`, `RecurPositional`, and `Return`.

Arithmetic on `Int`, `Float`, and `Bool` runs inline behind tag guards;
every other operand goes to the interpreter's own slow path, which handles
bindings, strings, and errors. A callee that is an initialized module `val`
is read at compile time and checked for identity at each call. A call with
exactly the arity of a compiled closure goes straight to its native entry;
any other call leaves native code.

### What each crate gains

- **`slug-vm`, behind `native-hooks`:** a counter per chunk (on the `Hint`
  type the index hints use) and one word for its compiled entry; the hook
  trait and the entry check in the dispatch loop; the list of native
  records and building `Frame`s from it; and safe functions for the
  arithmetic slow paths, clone and drop, reading a binding, building
  errors, the attention check, and building frames on exit.
- **`slug-native-tier`:** the lift from bytecode into SSA with a snapshot
  at every exit point; the check of which chunks qualify; lowering through
  Cranelift with every type unknown; the `extern "C"` shims and the entry
  and exit code (the crate's only `unsafe`); and one JIT module behind a
  mutex, since forks on other threads share programs.
- **Controls:** `SLUG_NATIVE=off|on|all` (`all` compiles every qualifying
  chunk at its first call, for differential testing),
  `SLUG_NATIVE_THRESHOLD` (calls and `recur`s before compiling; 1,000 to
  start, tuned by A/B), and a development flag that prints a chunk's
  bytecode, its IR, and why it was rejected. Nothing like that exists
  today, and debugging needs it.

### Steps

Each is committed on its own and passes the usual checks.

1. **Plumbing that compiles nothing:** the crate, the features, the
   counters, and the entry check. The A/B must show every workload within
   noise. If the counters cost anything, the plan changes before any
   compiler work. *Done (2026-11-18): `slug-vm`'s `native-hooks`
   feature counts each chunk's entries at the dispatch loop's frame check
   and at `recur`, and tells the installed `NativeTier` once a chunk is
   hot; `slug-native-tier` only counts hot chunks so far, and `slug`
   gains a `native` feature with `SLUG_NATIVE`, `SLUG_NATIVE_THRESHOLD`,
   and `SLUG_NATIVE_TRACE`. PGO A/B of `native` (tier on, counting)
   against `67d9d3e`, 9 rounds: tuned 0.99x, hold-out 0.99x, every
   workload within its spread (Crawl 0.96-0.99x, `fib` 1.02x, n-body
   1.04x at a 13% spread). The counters cost nothing measurable.*
2. **The lift,** with tests that lift every chunk of the conformance corpus
   and the clutches and check stack heights and snapshots against the
   verifier. *Done (2026-11-18): `slug-vm` gives the tier
   each chunk as verification sees it (`Program::verified_chunk`: the
   unpacked instructions with the verifier's own heights, stack effects,
   and successors, so the lift cannot disagree with it). `lift` builds the
   IR in one pass in bytecode order; locals a closure captures stay cells,
   and reading the caller's stack values is rejected. `Function::check`
   verifies single definitions, dominance, edge arity, and snapshots. Every
   chunk of the 158 repository sources that compile alone (3,067 chunks)
   lifts and checks, as does every chunk entered under `SLUG_NATIVE=all` in
   the clutch suites (2,531) and all 26 benchmarks; none failed.
   `SLUG_NATIVE_DUMP` prints the IR. `make test` failed one unrelated
   test while this step was checked, `typed_net`'s
   `serves_200_concurrent_clients_each_their_own_data`: some of its 200
   simultaneous clients are reset on connect (the macOS listen queue,
   `kern.ipc.somaxconn`, is 128). It fails the same way at step 1's commit,
   which passed it hours earlier, so it is flaky under this machine's state,
   not caused by the lift.*
3. **Lowering the subset,** until `fib`, `function-call`, n-body, and
   spectral-norm run natively. Split in two, because generated code needs a way
   out as soon as an operand leaves its fast path, so a single-frame exit
   moved here from step 4:
   - *3a, done (2026-11-19):* single-frame native code. A chunk compiles
     when it uses only number, boolean, and `nil` constants, locals,
     arithmetic and comparisons, branches, a positional `recur`, and
     `return`. Integer and float operands of one type take inline fast
     paths matching the VM's `numeric_binary`; any other operands go
     through the interpreter's own operation (`Vm::native_binary`). An
     overflow, a non-finite result, a failed operation, or a thread that
     needs attention at a `recur` leaves to the interpreter before the
     instruction, with the frame from its snapshot, so the interpreter
     raises the error with its usual trace. The VM enters native code at
     instruction 0 on a call or `recur`. Release build, native off against
     on: n-body's `advance` 20M times 1.17 s to 0.14 s (8.4x), an integer
     `recur` loop 20M times 0.72 s to 0.10 s (7.2x), spectral-norm
     2000x2000 0.48 s to 0.19 s (2.5x; its mixed and divided operands take
     the slow path). The whole `slug` test suite, the clutch suites, and
     every benchmark's output are unchanged with every qualifying chunk
     compiled (`SLUG_NATIVE=all`); one timing-dependent test now pins the
     interpreter. Only 40 of the clutch suites' 2,531 hot chunks qualify;
     the rest stop at constant-key map reads (629), captured locals (544),
     global reads (496), captures (245), other constants (210), and calls
     (121). PGO A/B against step 2's native build, 9 rounds: geomean
     1.02x tuned, 1.01x hold-out. The corpus's short runs (5–10 ms) pay
     for compiling at threshold 1000: n-body 0.91x, but spectral-norm
     1.12x and function-call 1.08x. The long runs (Crawl, Invaders, the
     HTTP server) are 0.99–1.02x, and Cranelift adds about 1 MB of RSS.
     Before tuning the threshold, find out whether the steady ~1% on
     programs that compile nothing comes from the entry check.
   - *3b, done (2026-11-19):* calls between compiled functions. A call of
     a module-level name (`CallGlobalPooled`) goes through `native_call`,
     which asks the VM for the callee (`Vm::native_callee`: an exact
     positional closure with native code, counted toward becoming hot) and
     runs its code directly, with no interpreter frame. The callee's
     closure is the only record a native activation keeps; the call span
     comes from the caller's frame when the interpreter rebuilds it. When
     there is no such callee, or native calls are deep (step 4 made this
     a stack budget), the caller
     exits before the call and the interpreter makes it. When a callee
     exits, its frame is recorded and every caller exits after its call,
     so the interpreter gets all the frames at once
     (`NativeOutcome::Exit`'s `frames`) and nothing native stays on the
     machine stack. Recursion past the depth limit continues in the
     interpreter, which can enter native code again a frame further on.
     Generated code reaches its slots from one frame address (Cranelift
     kept each slot's address in a register, saving ten registers a call),
     and a branch on a comparison takes the boolean directly. Release
     build, native off against on: `fib(32)` 474 ms to 108 ms (4.4x), 20M
     calls of `increment` from a `recur` loop 1,506 ms to 289 ms (5.2x).
     Every suite and benchmark output is unchanged under `SLUG_NATIVE=all`;
     52 of the clutch suites' 2,531 hot chunks now compile.
     `SLUG_NATIVE_DUMP` also prints each compiled chunk's machine code.
     A chunk whose entries from the interpreter mostly exit early or are
     declined settles after 64 entries and stays interpreted:
     `defer-errors`' loop compiles but calls a function that does not, so
     each iteration entered native code and left at the call, 1.15x slower
     until it settled. PGO A/B against 3a, 9 rounds: geomean 0.94x tuned,
     1.01x hold-out; `fib` 0.46x, `function-call` 0.70x, `defer-errors`
     1.01x, everything else within noise. The Crawl workloads' RSS moves
     between about 45 and 58 MB from run to run in both builds.
4. **Leaving native code, done (2026-11-19):** the exit across several
   native frames, the depth limit, and errors.
   Diagnostic snapshots are identical in both modes, and a test recurses
   past the depth limit. 3b built the all-or-nothing exit; this step made
   it safe and tested it:
   - The depth limit is a byte budget, 256 KiB of machine stack measured
     from the entry, rather than 256 calls: a function with many values
     has a large native frame, and 256 of them could overflow a fork
     thread's 2 MiB stack. Removing the budget makes deep recursion in a
     fork crash, which a test now catches.
   - A native call checks the thread's attention flag, as the interpreter
     checks it at every call, so a cancelled fork deep in native recursion
     stops. Without it a cancelled `fib(60)` ran on; a test now kills slug
     after 30 s and fails. The two checks cost about 4% on `fib(32)`
     (106 to 111 ms; 4.2x over the interpreter).
   - Tests compare stdout, stderr, and status in both modes for an
     overflow three native calls deep, an overflow under 5,000 frames
     that cross the budget repeatedly, and recursion 20,000 and 30,000
     deep in forks. The conformance corpus already runs under
     `SLUG_NATIVE=all` in the suite.
5. **Differential CI, done (2026-11-19):** the conformance corpus,
   `make test`, `make test-clutches`, and the diagnostic goldens under
   `SLUG_NATIVE=all`. `make test-native-all`, part of `make check` and so
   of CI's check job, runs the `slug` suite (which runs the conformance
   corpus and its exact diagnostics), the clutch suites, and
   `scripts/native-differential.py`, which runs each source benchmark with
   native code off and `all` and fails on any difference in output,
   diagnostics, or status. About 30 s plus the build on an M-series Mac;
   the benchmarks run in parallel. On macOS the suite hits the
   `typed_net` listen-queue failure as `make test` does.
6. **Measure, done (2026-11-19):** a PGO A/B of native on against native
   off, with hold-outs. PGO builds of 1ae7597 without and with `native`,
   9 interleaved rounds: geomean 0.95x tuned, 1.01x hold-out.

   | workload | interpreter | native | ratio |
   |---|---:|---:|---:|
   | `fib` | 26.4 ms | 12.6 ms | 0.48x |
   | `function-call` | 9.2 ms | 6.9 ms | 0.75x |
   | n-body | 6.5 ms | 5.8 ms | 0.90x |
   | spectral-norm | 5.6 ms | 6.1 ms | 1.11x |
   | `closure-accumulate` | 28.2 ms | 29.3 ms | 1.04x |
   | `higher-order` | 18.1 ms | 18.6 ms | 1.03x |
   | Invaders, Crawl, HTTP server | | | 0.98–1.01x |
   | hold-outs | | | 1.00–1.01x |

   The corpus kernels run 5–30 ms, of which about 5 ms is start-up and
   0.7–0.9 ms compiling, so their ratios understate the code: at release
   size (above) `fib` is 4.2x, 20M calls 5.1x, n-body's `advance` 8.4x,
   and spectral-norm 2.5x. `closure-accumulate` and `higher-order`, which
   compile nothing, are 3–4% slower, outside their 1% spread: the cost of
   counting entries on every call. The `native` build is 4.16 MB against
   2.84 MB (+1.32 MB).

   `SLUG_NATIVE_TRACE` now ends with a summary: chunks hot and compiled,
   time lifting and compiling, entries, activations that left early, and
   calls to functions without native code. The Crawl workloads compile 1
   to 5 of 24 to 127 hot chunks, in 1.7–3.4 ms; each leaves native code at
   an uncompiled call 64 to 128 times, until the chunk settles. Invaders
   compiles 8 of 65 and enters native code 249,302 times with 52 early
   exits. Resuming native code after an interpreted call would gain
   little; what Phase 2 needs for real programs is coverage (map reads,
   captures, global reads, closures).

   **Gate:** `fib`, `function-call`, and n-body pass at release size
   (4.2x, 5.1x, 8.4x); spectral-norm does not (2.5x), because its mixed
   integer and float operands and divisions take the slow path; none of
   the four reaches 4x in the corpus's short runs. All gain more than 2x,
   so Phase 1 does not stop. Invaders, Crawl, and the hold-outs are within
   noise, but two call-heavy tuned workloads pay 3–4% for the entry
   count. Compile time and binary size are recorded above.

### Gate

- `fib`, `function-call`, n-body, and spectral-norm at least 4x faster.
  The boxed prototype measured 10x on `fib` and 5.4x on `advance`, so 4x
  leaves room for the native records and the exits.
- `invaders-game`, the Crawl workloads, and every hold-out within noise.
  Phase 1 compiles little of them; this checks that the hooks and the exits
  cost nothing.
- Compile time and the binary size with `native` on, recorded.

If the four kernels gain less than 2x, Phase 1 stops until the cause is
understood. Step 6 also counts how often Crawl leaves native code at a call
to an uncompiled function, which decides whether Phase 2 needs a way to
resume native code after an interpreted call.

## Risks

- **Two engines to keep in agreement.** Every semantic change lands twice.
  The conformance corpus must run under both tiers in CI, with a mode that
  compiles every function at once.
- **Deoptimization correctness** is the classic source of JIT bugs; phase 3
  is gated on a differential test mode that deoptimizes at every guard.
- **Executable memory on macOS** needs `MAP_JIT` and per-thread write
  protection, which `cranelift-jit` handles; forks on other threads share
  compiled code read-only.
- **aarch64 limits found in the prototype:** Cranelift 0.113's JIT has no
  PLT and no non-PIC external calls on aarch64, so runtime helpers are
  called through their addresses (`call_indirect`), and some unordered
  float compares are not lowered in branches.
