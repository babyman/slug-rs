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
- [x] **Phase 1: baseline tier** for straight-line arithmetic, locals,
  branches, `recur`, and calls to module functions; everything else stays
  interpreted. Measure `fib`, `function-call`, n-body, spectral-norm, and
  `invaders-game`. *Done: see "Phase 1 plan" below.*
- [x] **Phase 2: the rest of the instruction set** through runtime helpers:
  index reads, collection construction, closures and captures, `match`,
  native calls. Measure the Crawl workloads and `http-server`. *Done:
  every hold-out faster; see "Phase 2" below.*
- [x] **Phase 3: type specialization with deoptimization.** *Closed
  without either: direct calls and cheaper returns met the gate, and
  specialization did not pay; see "Phase 3 result" below.*
- [ ] **Phase 4:** cleanup (`defer`/`onerror`) and tasks and forks in
  native frames. (Compiling off the VM thread landed in Phase 2.)

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

7. **Closing the gate.** Mixed integer and float operands and division
   now run inline. Two integers divide inline when the quotient is exact
   and continue as floats when it is not; a zero divisor, `i64::MIN / -1`,
   and a non-finite result still go to the interpreter, which reports
   them. Two numbers with at least one float convert as `numbers` does;
   equality of an integer and a float, which compares numerically, stays
   on the slow path. spectral-norm at 2000x2000 (release, not
   profile-guided) goes from 0.51 s to 0.04 s, about 12x (it was 2.5x),
   with identical output.

   On the entry-count cost: in ordinary release builds the `native` build
   with `SLUG_NATIVE=off` against the interpreter-only build is 1.00x
   tuned and 1.01x hold-out (`closure-accumulate` 1.02x, inside noise),
   and the tier on against off in one binary is 1.00x on
   `closure-accumulate`, `higher-order`, and `defer-errors`. Counting a
   settled chunk is one load and a compare; what is left of the 3–4% in
   the profile-guided comparison is the hooks' presence in the
   interpreter loop, not the counting, and is not pursued further.

   **Gate, again:** all four kernels pass at release size (`fib` 4.2x,
   `function-call` 5.1x, n-body 8.4x, spectral-norm about 12x).

### Phase 2 (2026-11-19)

Every blocker of every hot chunk the Crawl workloads, `invaders-game`, and
`http-server` leave interpreted, counted with a temporary `qualify` that
lists them all: 524 blocked chunks. Calls of closure values block 349,
global reads 298, captures 221, `MakeClosure` 126, captured locals 100,
and index reads about 380 between their sources; no single instruction
unlocks more than 12 chunks alone, so coverage comes in batches of
runtime operations, each through a VM `native_*` function that takes the
running closure (native callees have no VM frame, and may belong to
another module than the frame on top), reads through that closure's own
globals and slot cache and a fork's view, and returns `None` wherever the
interpreter would do more, so native code leaves before the instruction.

1. **Reads** (batch A): `GetGlobalPooled`, `GetCapture`, `GetIndex`,
   `GetIndexConstant`, `ReadIndex` and `ReadIndexConstant` from a local,
   a capture, or a global (in place, with the constant key's hint),
   constants of every kind, binary operations against a non-number
   constant, `Not` and `Negate` (inline for scalars), and `List`. Hot
   chunks compiled: the Crawl workloads 8–33 each (were 1–5), Invaders 18
   (8), `http-server` 10. Tier on against off in one release build, 11
   interleaved rounds: `crawl-ticks` and `invaders-game` 0.89x, the other
   Crawl workloads and `http-server` 0.98–1.01x, hold-outs 1.00x. Most
   newly compiled chunks still call closure values or make closures, so
   batch B is where they run natively end to end.
2. **Calls of values and global writes** (batch B): `CallPositional`
   enters a compiled closure's code as a call of a module-level name does,
   and `SetGlobalPooled` replaces a resolved module variable's value
   outside tasks with their own view. Compiled: crawl-crowd 67 of 127 hot
   chunks, `crawl-ticks` 46 of 72, Invaders 38 of 65. But compiling now
   cost 17–48 ms a run, about 1 µs per Cranelift instruction (37 of 39 ms
   in `define`; `opt_level = none` saves only a fifth), and `crawl-pages`
   ran 1.07x, Invaders 1.00x.
3. **Smaller code, compiled in the background.** A function with more
   than six owning slots releases them in one runtime call at each return,
   exit, and `recur`, instead of a check and call per slot (the largest
   chunk had 214 calls, most of them releases), which cut compile time by
   about a fifth. Then `SLUG_NATIVE=on` compiles on a thread of the
   tier's own: `NativeTier::hot` receives a `HotChunk`, which carries the
   program under the fork package's `Sendable` rules and installs the code
   from the compiler's thread; the chunk stays interpreted meanwhile.
   `SLUG_NATIVE=all` still compiles at the first entry, so the
   differential suites run everything natively. While any compile is in
   flight, exits are not held against a chunk: a compiled caller leaves
   at every call to a callee still compiling, and `function-call`'s loop
   had settled on those exits within its first 64 entries.

   Tier on against off in one release build, 11 interleaved rounds:
   `fib` 0.50x, `function-call` 0.49x, `invaders-game` 0.82x,
   `crawl-ticks` 0.88x, `crawl-history` 0.97x, every other Crawl
   workload, `http-server`, and the hold-outs 0.99–1.00x. Nothing is
   slower. The callees native code still cannot call are closures that do
   not compile (`MakeClosure`, struct copies) and builtins such as `len`:
   the next batch.
4. **Closures and captured locals** (batch C): `MakeClosure` and
   `MakeClosurePooled`, `SetCapture`, and a function a closure captures
   locals of. Its captured locals live in the activation's context rather
   than in slots, each a value until a closure captures it and the cell
   that closure shares after, as the interpreter promotes them; reads,
   writes, arithmetic, and index reads of one call the runtime; a `recur`
   gives the next iteration fresh ones; and an exit hands each to the
   interpreter as its frame's value or cell (`NativeLocal`). Compiled:
   crawl-crowd 80 of 127 hot chunks (67), `crawl-pages` 61 of 97 (47),
   Invaders 50 of 65 (38), `http-server` 37 of 78 (26). Tier on against
   off, one release build, 9 interleaved rounds: `fib` and
   `function-call` 0.49x, Invaders 0.83x, `crawl-ticks` 0.88x,
   `http-server` 0.95x, crawl-crowd 0.96–0.98x, the rest 1.00–1.01x
   (`crawl-pages` read 1.06x once and 1.00x over 15 rounds).

   The callees native code still leaves at are now mostly functions that
   build structs (`StructPooled`), append to lists (`ListAppend`), slice
   (`GetSlice`), or copy a module variable's struct (`CopyGlobalPooled`),
   and the builtin `len`, the most called single callee in the Crawl
   workloads and `http-server`. An append keeps the interpreter's
   in-place update only if native code moves a list it reads for the last
   time rather than cloning it, so `ListAppend` waits for last-use moves.
   A builtin that fails cannot be retried by the interpreter, which would
   repeat its effects, so a direct builtin call needs the failure handed
   over rather than redone. `UpdateCapture` (`total = total + x` on a
   captured `var`) needs the same moves.
5. **Operations that build values, last-use moves, and builtins** (batch
   D). `ListAppend`, `ListPrepend`, `GetSlice`, `Struct`, `StructCopy`,
   `CopyGlobalPooled`, and `UpdateCapture` each make one call to the VM's
   `native_operation`, with a table of the operands' slots: no code is
   generated to stage them. An operand the instruction reads for the last
   time moves rather than being cloned, so a list nothing else holds is
   appended to in place, as the interpreter's `MOVE_LOCAL` reads allow; a
   value is read for the last time when no later instruction or edge of
   its block, and no block live after it, uses it, a snapshot's operand
   stack counting as a use and its locals not (code that reads a local
   later uses its value there). An operation that cannot run hands its
   operands back, the moved ones to their slots, and native code leaves
   before it. Without liveness, a branch that returns a list kept every
   `recur(n - 1, acc :+ n)` from moving it: 300,000 appends took 117 s
   against the interpreter's 0.13 s; with it, 0.07 s.

   Calls of builtins (`Value::Native`, such as `len` and `println`) run
   from native code. A builtin that fails has had its effects, so the
   interpreter must not call it again: native code leaves past the call
   with the failure (`NativeFailure`), and the interpreter reports it in
   the restored frame, with the trace it would have given.

   Compiled: crawl-crowd 97 of 127 hot chunks (80), `crawl-pages` 76 of
   97 (61), Invaders 61 of 65 (50), `http-server` 49 of 78 (37). Tier on
   against off, one release build, 11 interleaved rounds: `fib` 0.52x,
   `function-call` 0.50x, Invaders 0.78x, `crawl-ticks` 0.90x,
   `http-server` 0.91x, `crawl-pages` 0.92x, crawl-crowd 0.96x, the rest
   0.99–1.00x. Exits rose with coverage (Invaders 51,515 activations left
   early, `tiny-interpreter` 32,239), nearly all at calls of callees
   without native code: the next thing to measure.
6. **Matches, defaults, throws, and maps** (batch E). A
   `MatchCasePooled` calls the VM's `native_match`, which writes the value
   and what the pattern binds to the instruction's results (consecutive
   slots) and says whether it matched, which picks the block's edge.
   Native code is entered only when every parameter was passed (the
   interpreter declines a frame whose call left one out), so
   `JumpIfProvided` always jumps. A block that ends in `throw`, a match
   failure, or `not implemented` leaves to the interpreter before it,
   which raises it. Map literals are one more `native_operation`.
7. **Fewer exits** (batch F), found by counting exits by instruction.
   A module variable a function only writes was never in its slot cache
   (the interpreter writes by name and caches only reads), so native code
   declined every such write: now native code resolves a name's slot as
   the interpreter's read does, and caches it. `~` compiles. Typed
   natives (`DeclaredNative`) are called directly, their resources
   checked as the interpreter checks them, an argument check failing
   before the call and a result check after it, as a failure; and so is
   `int`, which has no effects, so one that fails leaves before the call.
   A `+` or `-` whose left operand is read for the last time hands it to
   the VM when the operation cannot fail, so a map or list nothing else
   holds is updated in place: map literals compiling in batch E had made
   `word-frequency` 4.63x slower, `counts + {[key]: n}` copying the map
   each time. Liveness is now bit sets, computed when a value first may
   move.

   Exits fell from 94,097 to 15,447 a run in Invaders and from 51,538 to
   5,478 in `http-server`; Invaders compiles all 67 hot chunks. Tier on
   against off, one release build, all 25 benchmarks, 7 interleaved
   rounds: Invaders 0.57x, `function-call` 0.49x, `word-frequency` and
   `fannkuch-redux` 0.52x, `fib` 0.53x, `crawl-ticks` 0.79x,
   `http-server` 0.81x, `crawl-history` 0.89x, `crawl-pages` 0.93x,
   crawl-crowd 0.94x, every other 0.98–1.02x.

   **The PGO A/B (2026-11-19) does not close Phase 2.** PGO builds of
   19abce9 without and with `native`, 9 interleaved rounds: geomean
   0.91x tuned, 0.99x hold-out; Invaders 0.70x, `fib` 0.62x, but
   `higher-order` 1.06x and the hold-out `tiny-interpreter` 1.12x (spread
   5%). PGO speeds the interpreter far more than native code, and the
   quick checks above, on plain release builds, overstated the gains: the
   crawl-crowd quick check read 0.94x against 1.01x here. The `native`
   build is 4.92 MB against 2.84 MB, and resident memory grows 2–11 MB
   (`tiny-interpreter` 11.6 to 24.1 MB).

8. **Entering native code at the call.** `higher-order`'s cost was per
   entry (12% at every length, nothing at start-up): the interpreter
   called `slug.std`'s `map`, `filter`, and `reduce`, which stay
   interpreted, and they entered three compiled lambdas 105,329 times,
   each through a full interpreter call: a frame pushed, the locals moved
   into a fresh vector, the code run, the frame popped, and the dispatch
   loop's lookup of the active code redone. For a three-instruction
   lambda that costs more than native code saves. Now an exact positional
   call of a closure already compiled runs its native code at the call,
   with the arguments from the operand stack through a vector the VM
   keeps, and makes a frame only if the code leaves, from the state it
   leaves (`native_resumed` keeps the interpreter from entering it again
   at its first instruction).

   PGO A/B against the same interpreter build, 9 rounds: geomean 0.90x
   tuned, 0.98x hold-out; `higher-order` and `closure-accumulate` 0.92x,
   crawl-history 0.90x, crawl-game 0.91x, crawl-crowd 0.97x, Invaders
   0.70x, `fib` 0.62x, hold-outs `text-wrap` 0.86x, `expression-evaluator`
   0.96x, `graph-search` 0.98x, crawl-pages 1.01x, and `tiny-interpreter`
   still 1.09x (spread 2%). `string-building` 1.04x, `defer-errors`
   1.03x, and `shared-append` 1.02x are the tuned workloads still slower.

9. **`tiny-interpreter`, rotated out of the hold-outs.** It stayed 1.09x,
   so it became a tuned workload and `stack-machine` (syntax trees
   compiled to instruction lists and run on a stack machine in Slug) the
   hold-out in its place; that workload's interpreter time was seen once,
   in the bench run that checked its registration, but no comparison of
   it was. A profile of 25 runs of the Tiny program found dropping values
   at 734 samples with the tier on against 198 off: a function with more
   than six owning slots released every one at each return, exit, and
   `recur` through the full `Value` drop, `nil`s and numbers included, and
   the Tiny evaluator is one `match` with hundreds of them, few of which a
   call writes. `native_release` now reads each slot's tag and drops only
   a counted one: the long run went from 7.95 s against 7.66 s
   interpreted to 6.60 s. Also: `TryMatchPooled` (destructuring) and
   `InterpolatePooled` compile, so all of the Tiny program's hot chunks
   do, and a call from native code counts as 32 entries toward its
   callee's threshold, so the callees of compiled code compile sooner.

   PGO A/B of the tuned workloads against the same interpreter build, 9
   rounds: geomean 0.86x; `fib` 0.57x, Invaders 0.67x, `function-call`
   0.77x, crawl-ticks 0.81x, `word-frequency` 0.85x, crawl-game,
   crawl-history, and `http-server` 0.89x, `higher-order` 0.93x,
   `string-building` 0.94x, crawl-crowd 0.95x, `tiny-interpreter` 1.02x,
   `shared-append` 1.02x, and `defer-errors` 1.05x. Releasing every owning
   slot is still the largest native cost in the Tiny evaluator.

10. **`graph-search`, rotated out of the hold-outs.** The full A/B of
    item 9 had the hold-outs at 0.91x geomean, `stack-machine` 0.73x, but
    `graph-search` 1.29x, against 0.98x after item 8. Counting calls from
    native code singly again left it at 1.28x, so the weighting was not
    the cause; that was reverted anyway, the tuned workloads being the
    same without it. `graph-search` became a tuned workload, to be
    profiled, and `queens-knapsack` (eight queens by backtracking, a 0/1
    knapsack over lists) the hold-out in its place. Its output was checked
    by running it directly, with the tier off, on, and on for every
    chunk; no timing of it was seen.

    The cause was quadratic: compiling destructuring let `search`
    compile, and native calls copied every argument, so the `seen` map
    `search` passes `visit` for the last time had two owners there, and
    `visit`'s first insert copied all of it, once a node. The
    interpreter's calls move a local read for the last time. Native calls
    now move an argument they read for the last time, by the same
    liveness as operations, and move it back if the call is declined
    (`native_call` drops only the arguments it copied). Twenty times the
    nodes went from 7.0 s with the tier on against 1.4 s off to 1.48 s
    against 1.52 s.

    PGO A/B against the same interpreter build, 9 rounds: geomean 0.88x
    tuned, 0.83x hold-out. Hold-outs: `stack-machine` 0.74x, `text-wrap`
    0.77x, `queens-knapsack` 0.83x, crawl-pages 0.91x,
    `expression-evaluator` 0.92x; none slower. Tuned: `graph-search` 0.93x
    (from 1.28x), `fib` 0.56x, Invaders 0.66x, `function-call` 0.74x,
    crawl-ticks 0.80x, `tiny-interpreter` 0.99x; `defer-errors` 1.03x,
    `shared-append` 1.02x, and spectral-norm 1.01x and 1.02x (inside its
    spread) are the tuned workloads still slower.

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

## Phase 3 plan (2026-11-20)

Phase 2 closed with every hold-out faster (geomean 0.83x) and the tuned
workloads at 0.88x, but the kernels far from Phase 1's 4x: `fib` 0.56x,
`function-call` 0.74x, n-body 0.92x, spectral-norm 1.01x.

### Where `fib`'s time goes

A profile of `fib(38)` with the tier on (a release build with symbols, one
second, ~420 samples on the VM thread) has about two thirds of the time in
calling, not in arithmetic or tag checks: `native_call` 101 samples,
`Compiled::enter` 52, `Vm::native_entry` 39, `native_slot` 23, the
context's exit record dropped 19, the callee cloned and dropped 26, the
downcast to `Compiled` 8. Returning cost another 68 in `native_release`,
since `fib` has seven owning slots, one more than are checked inline.

Each native call resolves its callee as the interpreter does, through the
module's slot table, clones it, finds its chunk's code through the counted
`NativeSlot`, clones that `Arc`, downcasts it, and builds a fresh context.
The boxed prototype's 10x called the compiled function directly.

### Steps

1. **Cheaper returns.** One inline test of every owning slot's tag, with a
   call to drop them only when one holds a counted value, in place of the
   call each return made when a function had more than six. `fib(35)`
   (release, not profile-guided): 0.67 s to 0.57 s.
2. **Direct calls between compiled functions.** A call site remembers the
   program and chunk it last entered and that chunk's `Compiled`, keyed so
   a freed closure's address cannot alias another chunk's code; the
   generated code checks the callee against it and calls the entry
   directly, with the context built in its own frame, and falls back to
   `native_call` otherwise. `Value` is not `Send`, and compiled code is
   shared between threads, so the cache holds no `Value`: only the
   program's address, the chunk, and the code, which lives as long as the
   process.
3. **Type facts.** A forward pass over the IR finds the values that are
   always an `Int` or a `Float` (constants, the results of a guarded
   arithmetic fast path, parameters after their first guard), so their
   tag checks and their release go.
4. **Specialization with deoptimization.** Such values stay in registers
   between guards; a guard that fails writes them back to their slots and
   leaves through the exit tables that exist already. Gated on a
   differential mode that fails every guard once (the risk below).
5. **Unboxed arguments** between specialized functions, with the boxed
   entry kept for every other caller.

Each step is measured on `fib` and the kernels, then on the full PGO A/B
with the hold-outs; a step that does not pay is reverted.

### Progress

1. *Done.* Cheaper returns, as above.
2. *Done.* Direct calls. A per-thread memo of the callee in the runtime
   bounded the runtime-side saving at about 9% (`fib(35)` 0.57 s to
   0.52 s), so the check moved into generated code. Each module table now
   carries a stamp, taken from one process-wide counter whenever an entry
   is set or becomes a variable's cell, so no other table, even one at a
   freed table's address, can share it. A call to a module-level name
   records, in the caller's `Compiled`, the stamp, the callee's entry and
   `Compiled`, the callee's own table's stamp, and its own clone of the
   closure, when the name holds a closure that captures nothing in the
   table itself (not a variable's cell) and its code makes no cells.
   Generated code then checks the record, the stamp, the stack budget, the
   thread's attention flag, and that no argument is a module binding,
   builds the callee's context in its own frame (the context is now plain
   data: its cells and its exit are pointers), and calls the entry. A
   callee that exits is recorded as `native_call` records it, by
   `native_call_exited`. `fib(35)`: 0.57 s to 0.25 s, against 1.02 s
   interpreted. No program run reaches a stamp change of a recorded name,
   since a program's own definitions precede its calls and a variable's
   assignments go to its cell; a REPL's redefinitions would.

   The first A/B (p9: tuned 0.87x, hold-out 0.84x, `fib` 0.42x) put
   `tiny-interpreter` at 1.08x. Its calls mostly take the slow path (the
   callee is a closure that captures), and each one recorded the site
   again, allocating a fresh record. A site whose record still holds is
   now left alone, and a return with more than 32 owning slots calls
   `native_release` without testing them inline. p10 against p6-base:
   tuned 0.87x, hold-out 0.83x; `fib` 0.40x, `binary-trees` 0.77x,
   `tiny-interpreter` 1.00x, `defer-errors` 1.06x; hold-outs
   `stack-machine` 0.72x, `queens-knapsack` 0.86x, `text-wrap` 0.79x,
   `crawl-pages` 0.90x, `expression-evaluator` 0.90x. The gate's clutch
   run with `SLUG_NATIVE=on` hung once after every suite reported, with
   `closesAStreamWhoseClientLeaves` failed, and `typed_net`'s 200-client
   test saw one refused connection. Neither was the tier's: both were
   tests bounded by count where a loaded machine needs time. The HTTP
   test failed and hung 2 in 16 runs with every core busy and the tier
   off (0 with it on); it gave up after 200 sends, then left its server
   waiting on the body channel and the listener. It now sends for up to
   five seconds and closes both on the way out (0 in 32). The 200-client
   test retried a refused connect for about a second, and now for up to
   20 (10 of 10 under the same load).
3. *Tried, reverted.* Type facts as planned are unsound: a slot can hold
   a module binding, which the VM's operations resolve before computing,
   so `n < 2` succeeding says nothing of `n`'s tag. Facts hold only where
   a slot is written: number constants, comparisons, and arithmetic on
   operands already known. That leaves almost nothing without a check, so
   the step was folded into a first piece of step 4 without
   deoptimization: each function whose parameters an operator reads was
   lowered twice, the entry (and each `recur`) checking that those
   parameters hold an `Int` and taking a copy that knows it (no tag tests
   on them or on integer results, no binding checks on such arguments,
   copies instead of clones on edges), or else the general code.
   `fib(38)` did not move (1.05 s both ways): with direct calls, its time
   is the call itself, the callee's context built field by field, the
   guards, eight saved registers and a 432-byte frame, not tag tests. An
   integer `recur` loop gained 7% (0.21 s to 0.195 s); one whose
   accumulator becomes a float lost 3%, failing the entry check every
   iteration. The PGO A/B against p10: tuned 1.02x, hold-out 1.01x,
   n-body 1.07x, `queens-knapsack` 1.05x, more memory everywhere from
   the doubled code. The patch is kept outside the repository.

   Found on the way: integer `%` has no inline path, so a loop using it
   calls the runtime each time; and native code's own instructions are
   now nearly all of `fib`'s profile, so the next gains for calls are a
   smaller context and fewer guards per call, and for the kernels, reads
   guarded where they happen (step 4 proper).
4. *Cheaper calls (2026-11-20).* `fib(38)` makes about 126 million calls
   in a second, some 30 cycles each, so a call's cost is its chain of
   dependent loads and stores, not its instruction count: a value passes
   through memory several times on its way (caller's slot, the argument
   area, the callee's slot; the callee's result, the context, the
   caller's slot), each a store-to-load forwarding delay. Dropping two
   guards that the core runs alongside the chain (the record's null test
   and its table pointer, a stamp standing for its table) did nothing,
   and was reverted. What paid, release `fib(38)`, 7 interleaved rounds:
   - A directly called function writes its value into the caller's
     result slot, the context's `ret` now a pointer: 1.084 s to 0.943 s.
   - Each word of a `Value` in an alias region of its own: Cranelift
     forwards a store to a load only from the last store in the region,
     so a tag load never saw a tag store behind a payload's. 0.94 s to
     0.90 s.
   - Integer `%` inline (`srem`; a divisor of 0 or -1 and floats go to the
     interpreter's operation): a 3M-step LCG loop, 0.32 s interpreted, 0.03
     s native.

   PGO A/B of the three against a fresh build of `52b921b9`: tuned 0.99x,
   hold-out 1.00x, `fib` 0.93x. Two PGO builds of one source measure
   1.00x against each other, so the A/Bs above are not build noise.

   Tried and reverted: each slot's words as Cranelift variables, defined
   by every store, so a load takes the stored value even across joins,
   with slots the runtime may have written (those whose address a call
   was given, and the results of runtime operations) reloaded after it,
   and the values still read reloaded after every call so no variable
   lives across one. An integer `recur` loop gained 30% (0.214 s to
   0.150 s) and `fib` 3%, but the real programs lost 3-4% and the PGO
   A/B came out tuned 1.01x, hold-out 1.05x, with up to 13 MB more
   memory: the code grows with a reload per live value after each call,
   which Cranelift does not drop when unused. Registers for values belong
   with specialization, where a value's type is known and it need not
   also live in the frame.

### Gate

- The kernels at least 4x faster on compute time, startup subtracted.
  n-body and spectral-norm run 1 to 2 ms past about 6 ms of startup, too
  little for a ratio, so they are also measured at 20 times their input
  outside the suite, whose workloads stay as they are.
- Every hold-out within noise or faster, and the tuned geomean no worse.

### Phase 3 result (2026-11-20)

The gate is met. On compute time, at inputs large enough that startup
does not count (release builds; the 20x inputs the gate names still run
under 60 ms interpreted, too short to time):

| Kernel | Interpreted | Native | Speedup |
|---|---:|---:|---:|
| `fib(38)` | 8.86 s | 0.90 s | 9.8x |
| `function-call`, 30M calls | 2.51 s | 0.21 s | 12x |
| n-body, 20M steps | 1.18 s | 0.11 s | 10.7x |
| spectral-norm, 50,000 rows | 0.64 s | 0.06 s | 10.7x |

PGO A/B of the tier against the interpreter at `0c64c9ee`, nine
interleaved rounds: tuned 0.87x, hold-out 0.84x. Hold-outs: `stack-machine`
0.74x, `text-wrap` 0.78x, `queens-knapsack` 0.88x, `expression-evaluator`
0.91x, `crawl-pages` 0.92x. Of the real programs, `invaders-game` 0.71x,
`crawl-ticks` 0.83x, `http-server` 0.87x, `crawl-history` 0.89x,
`crawl-game` 0.91x, `crawl-crowd` 0.93x. Still slower: `defer-errors`
1.06x and `shared-append` 1.02x, neither traced yet (`defer` and
`onerror` are not compiled, Phase 4). Peak RSS is 2.5 to 14 MB higher, the
compiler and its code.

What paid was calling: direct calls between compiled functions, a single
inline test of the owning slots on return, results written into the
caller's slot, and an alias region per word of a `Value`. Type facts
without a check turned out unsound (a slot can hold a module binding),
and an integer version of each function, the check made once at entry,
did not pay (1.02x), so deoptimization was never needed. The kernels'
suite ratios stay near 1.00x because their work is 1 to 2 ms past about
6 ms of startup; their speed shows only at scale.

Where the real programs' native time goes is runtime work (map and index
reads, clones and drops, binding resolution; see "Gate 0"), which type
guards would not remove; it would serve both tiers.

### The slower workloads (2026-11-21)

`defer-errors` was the one workload slower with the tier on (1.04x at
2M iterations, 1.06x in the A/B). Its functions with `defer` stay
interpreted, but the deferred bodies themselves (`{ done = done + 1 }`,
`err.code`) compile, and cleanup ran each through an interpreter frame
and then the frame-entry path into native code: about 13 ns lost a run
of a two-instruction body. Leaf functions called from the interpreter
did not show it, since an exact positional call enters native code
without a frame, which wins even for `fn(x) { x }`. Cleanup now does the
same: a compiled action runs without a frame, its value dropped or, for
`onerror`, recovered with as a native function's is; if it leaves to
the interpreter, the frame it leaves is pushed then, marked as a cleanup
frame. PGO A/B against `919339bd`, 9 rounds: `defer-errors` 0.91x,
tuned 1.01x, hold-out 1.01x, the 1.05x kernels 1.00x on a 21-round
rerun.

`bench-ab.py --bin` on the same two binaries measured `defer-errors`
1.00–1.01x four times, though other timings of them gave 0.91–0.92x. It
was not the background compiler: a timeline of each run shows the
deferred body installed within about 1 ms of becoming hot, 4 ms into a
40 ms run. It was the size of the environment, which moves the initial
stack and so every frame's alignment. Padding the environment by 0 to
128 bytes in steps of 8, 12 interleaved runs each: the candidate is
0.92–0.94x at most sizes, 1.01–1.02x at 16–24 bytes (where the script's
environment fell), and 0.85x at 32–40 bytes, where the base slows to
38 ms instead. A single A/B measures every workload at one stack
alignment, so a workload's ratio can be off by several percent either
way; the geomeans average it out across workloads.

`shared-append` is 1.005x at 30,000 steps, inside its spread: its time
is the runtime copying lists, which both tiers share.

### Before merging

- **CI.** The decision record has the conformance corpus and the clutch
  suites run under both tiers in CI, including the mode that compiles
  every chunk at once. CI's `check` job runs `make ci`, which is
  `make check`: `test-native-tier`, `test-native-on`, `test-native-all`,
  and the differential, on Linux x86-64.
- **The default build.** *Decided 2026-11-20:* `native` is a default
  feature of `slug`, and `SLUG_NATIVE` is `on` when unset, except on
  Windows, where native code has not run yet (the VM's thread there has a
  1 MB stack, which native frames share): it stays `off` there until
  tested. `make test` sets `SLUG_NATIVE=off`, so it still checks the
  interpreter alone, and `make test-native-on` runs the slug suite as
  programs run by default.
- **Known gaps.** Functions with `defer`/`onerror` blocks (their
  deferred bodies compile) and tasks and forks in native frames are
  Phase 4. The fork
  conformance test `cancel-releases-waits` fails under load on `master`
  too (25 of 72 runs there, 33 of 72 here), so it predates this branch.

## Risks

- **Two engines to keep in agreement.** Every semantic change lands twice.
  The conformance corpus must run under both tiers in CI, with a mode that
  compiles every function at once.
- **Deoptimization correctness** is the classic source of JIT bugs; any
  phase that adds deoptimization is gated on a differential test mode that
  deoptimizes at every guard. Phase 3 closed without it.
- **Executable memory on macOS** needs `MAP_JIT` and per-thread write
  protection, which `cranelift-jit` handles; forks on other threads share
  compiled code read-only.
- **aarch64 limits found in the prototype:** Cranelift 0.113's JIT has no
  PLT and no non-PIC external calls on aarch64, so runtime helpers are
  called through their addresses (`call_indirect`), and some unordered
  float compares are not lowered in branches.
