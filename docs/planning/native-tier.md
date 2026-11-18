# A native-code tier

## Purpose

Decide whether, and how, Slug compiles hot functions to machine code, to
take it past CPython and toward the JVM on the programs where the
interpreter is the limit. This is a scoping plan: it records why the
interpreter alone will not get there, a measured bound on what native code
could reach, and the decisions and phases a native tier would need. Nothing
here is built yet.

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
- [ ] **Decide:** the exemption from `unsafe_code`, the Cranelift
  dependency, and desktop-only scope (a decision record).
- [ ] **Phase 1: baseline tier** for straight-line arithmetic, locals,
  branches, `recur`, and calls to module functions; everything else stays
  interpreted. Measure `fib`, `function-call`, n-body, spectral-norm, and
  `invaders-game`.
- [ ] **Phase 2: the rest of the instruction set** through runtime helpers:
  index reads, collection construction, closures and captures, `match`,
  native calls. Measure the Crawl workloads and `http-server`.
- [ ] **Phase 3: type specialization with deoptimization.**
- [ ] **Phase 4:** cleanup (`defer`/`onerror`), tasks and forks in native
  frames, compiling off the VM thread.

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
