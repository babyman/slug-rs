# Source benchmark baseline — 2026-09-16

This is the initial end-to-end performance baseline for the Slug VM
optimization work. It is local evidence for comparing later runs on this
machine; it is not a portable performance claim or a timing-sensitive test.

## Environment

| Field                              | Value                                      |
|------------------------------------|--------------------------------------------|
| Slug runtime revision              | `e9ae658ef9553dc6b5c72c29323a2bfd3250de98` |
| Slug executable                    | `target/release/slug`                      |
| Slug version                       | `slug-vm 0.1.0`                            |
| CPython executable                 | `/usr/bin/python3`                         |
| CPython version                    | `Python 3.9.6`                             |
| Warmup runs per runtime/workload   | 3                                          |
| Timed samples per runtime/workload | 15                                         |
| Reported statistic                 | Median wall-clock duration                 |

`make bench-source` builds the Slug release executable, then starts one fresh
Slug or CPython process per sample. Timings therefore include process startup,
source loading and compilation, and program execution. Each workload pair is
validated against checked expected output before timing.

## Results

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  152.707 ms |      32.870 ms |          4.65x |
| n-body        |   54.808 ms |      23.096 ms |          2.37x |
| spectral-norm |   17.603 ms |      21.243 ms |          0.83x |
| binary-trees  |  157.917 ms |      30.097 ms |          5.25x |

Values below `1.00x` mean Slug was faster in this run. The workload corpus is
Benchmark-Game-inspired rather than an upstream corpus port; see
[`benchmarking.md`](benchmarking.md) for its current scope and invocation.

## Interpretation

The first tuning targets are call/frame overhead and allocation-heavy
collection workloads. The nested numeric workload does not justify prioritizing
floating-point dispatch changes without more focused evidence. Future reports
should append a dated comparison rather than replace this baseline.

## Exact positional closure calls

After adding the exact positional closure-call fast path, a 15-sample run on
the same laptop reported the following medians. The worktree was dirty because
the VM change was not yet committed, so the displayed base revision (`8ef4f34d48abb7cffaefdb22ca2cd7e1b8b614de`) alone
does not identify this
implementation state. The runner now reports that condition explicitly.

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  137.238 ms |      28.343 ms |          4.84x |
| n-body        |   54.405 ms |      24.251 ms |          2.24x |
| spectral-norm |   18.180 ms |      21.981 ms |          0.83x |
| binary-trees  |  141.585 ms |      32.553 ms |          4.35x |

The ratio is sensitive to the independently measured CPython median; compare
the absolute Slug medians as well. The internal `ordinary-calls-200` benchmark
fell from approximately 329 ms to 303 ms for 1,000 runs, and all 201,000
ordinary calls took the exact positional path.

## Compact positional calls and frame-local metrics

`CallPositional(argc)` now records the compiler's already-known positional
shape directly in private bytecode. A 15-sample local run, again from a dirty
worktree based on `7146ffa83786d8662694dcc7a6ffda02974ef6f4`, produced:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  137.040 ms |      28.746 ms |          4.77x |
| n-body        |   55.225 ms |      24.500 ms |          2.25x |
| spectral-norm |   18.410 ms |      22.494 ms |          0.82x |
| binary-trees  |  140.216 ms |      31.478 ms |          4.45x |

The source-level timing change is within ordinary local variation, so this
slice is primarily a representation cleanup and measurement boundary. Its
new internal counters are more decisive: `ordinary-calls-200`, run 1,000
times, creates 202,000 frames but 402,000 frame-local vectors with total
capacity 1,604,000 and 602,000 argument values placed into locals. The extra
200,000 local vectors come from `recur` replacing locals without allocating a
new frame. A stack-window or reusable-frame-local experiment should therefore
measure whether it can eliminate that movement while preserving captures,
cleanup, suspension, and diagnostic-frame semantics.

## Direct-local `recur` reuse

`recur` now reuses its existing local vector only when every slot remains a
direct value. A frame whose local has been promoted to a captured binding cell
still receives a replacement vector, preserving the earlier iteration's cell
identity for an escaping closure.

A 15-sample local run from the dirty worktree based on
`152e535339ae5510e0d70ffeeaf2dc3a7967811f` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  137.396 ms |      28.301 ms |          4.85x |
| n-body        |   54.254 ms |      23.694 ms |          2.29x |
| spectral-norm |   18.047 ms |      21.559 ms |          0.84x |
| binary-trees  |  138.314 ms |      30.788 ms |          4.49x |

The external figures remain within run-to-run variation, but the internal
accounting demonstrates the representation change precisely. Across 1,000
`ordinary-calls-200` runs, frame-local vectors fell from 402,000 to 202,000
and their total capacity from 1,604,000 to 804,000. All 200,000 `recur`
restarts reused direct locals; the 602,000 argument values written to locals
were unchanged. Conversely, `closures-retained-128` recorded 12,800
replacements and no reuse, demonstrating the captured-cell safety boundary.

## Exact positional stack-to-local initialization

Exact positional closure calls now construct their final frame-local vector
directly from the call's operand-stack values. This removes the temporary
`Vec<Value>` previously built solely to feed `frame_locals`; generic calls,
including defaults, named arguments, spreads, variadics, and native calls,
retain their existing binder path. Exact selected closures retain their
live-binding identity validation before taking this path.

A 15-sample local run from the dirty worktree based on
`548c35c140d376482e07c1eb5f69e3e6baca5ed8` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  131.071 ms |      28.371 ms |          4.62x |
| n-body        |   55.072 ms |      24.647 ms |          2.23x |
| spectral-norm |   18.206 ms |      22.014 ms |          0.83x |
| binary-trees  |  132.982 ms |      30.870 ms |          4.31x |

The internal `ordinary-calls-200` benchmark now records zero temporary
closure argument vectors and 202,000 exact stack-to-local initializations
over 1,000 runs. Its final local-vector capacity falls from 804,000 to
202,000 slots because the final vector is allocated at the chunk's actual
local count, rather than inheriting and growing an intermediate vector's
capacity. The source call workload improved from 137.396 ms to 131.071 ms in
these samples; the allocation evidence is the more stable result.

## All-supplied frame parameters

Frames now represent an ordinary exact call's parameter state as `All` rather
than allocating `vec![true; arity]`. A bitmap remains only where argument
binding can distinguish supplied from defaulted parameters, including the
full-binding `recur` path. `JumpIfProvided` preserves its prior behavior by
asking the representation whether its parameter was supplied.

A 15-sample local run from the dirty worktree based on
`c33f2ec796aafe7e7da0e161fef2635444fc94a3` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  128.403 ms |      28.397 ms |          4.52x |
| n-body        |   54.632 ms |      23.910 ms |          2.28x |
| spectral-norm |   18.405 ms |      22.078 ms |          0.83x |
| binary-trees  |  131.489 ms |      31.546 ms |          4.17x |

The internal counters show no provided bitmap for exact calls. The
`ordinary-calls-200` workload still records 201,000 bitmaps with total
capacity 400,000 because its 200,000 `recur` restarts use the generic binding
rules; this makes the remaining cost explicit rather than hiding it. Its
elapsed time fell from about 297.5 ms to 283.2 ms for 1,000 runs in these
local samples.

## Derived frame diagnostic names

VM frames no longer retain a cloned function-name `String`. Each already owns
the program and closure chunk needed to recover that name when constructing a
runtime diagnostic, so the lookup now occurs only on the error path. Existing
function-name and source-call-site diagnostic tests protect the behavior.

A 15-sample local run from the dirty worktree based on
`f251761af6736102d244b045bad11679f2bfa6e0` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  125.684 ms |      27.902 ms |          4.50x |
| n-body        |   54.309 ms |      23.346 ms |          2.33x |
| spectral-norm |   18.021 ms |      21.673 ms |          0.83x |
| binary-trees  |  125.912 ms |      30.567 ms |          4.12x |

`Frame` shrank from 184 to 160 bytes. The internal `ordinary-calls-200`
workload fell from about 283.2 ms to 278.4 ms for 1,000 runs in these local
samples. This leaves eager defer-scope allocation as the next small
frame-entry cost; compact call-site diagnostics need a separate design pass.

## Lazy defer-scope storage

Frames now track lexical scope depth without allocating cleanup storage. The
first executed `defer` materializes entries for the active root and nested
scopes; frames that never register a deferred action retain no scope stack.
This preserves cleanup ordering through nested scopes, `recur`, and error
recovery while removing an allocation from ordinary calls.

A 15-sample local run from the dirty worktree based on
`c680103d358f087e55901389b253e983b2e6af76` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  114.994 ms |      27.206 ms |          4.23x |
| n-body        |   50.996 ms |      22.998 ms |          2.22x |
| spectral-norm |   17.091 ms |      21.328 ms |          0.80x |
| binary-trees  |  117.304 ms |      30.055 ms |          3.90x |

The internal counters report zero defer-scope stacks across ordinary call and
collection workloads, while `deferred-cleanup` materializes exactly 1,000
single-entry stacks in 1,000 runs. A compact `u32` depth field keeps `Frame`
at 160 bytes. The external function-call workload improved from 125.684 ms to
114.994 ms in these samples; as usual, repeated local runs are needed to
separate the durable effect from system variation.

## Restricted positional `recur`

The compiler now lowers a syntactically all-positional `recur(...)` to compact
`RecurPositional` bytecode. At runtime, that opcode restarts directly when the
active function's arity matches and it has neither defaults nor a variadic
parameter. Otherwise it retains the existing generic binder, so omitted
defaults, variadics, and malformed bytecode preserve their checked behavior.

A 15-sample local run from the dirty worktree based on
`47ccb3c018763a5549079fa85e548774a205ae71` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |  102.301 ms |      29.413 ms |          3.48x |
| n-body        |   43.917 ms |      24.755 ms |          1.77x |
| spectral-norm |   15.763 ms |      23.025 ms |          0.68x |
| binary-trees  |  120.173 ms |      34.963 ms |          3.44x |

The internal `ordinary-calls-200` workload recorded 200,000 exact positional
restarts and zero generic recur bindings across 1,000 runs, while keeping its
existing all-direct local-vector reuse. In these samples it completed in
227.240 ms, compared with about 266.456 ms in the preceding lazy-scope run.
The new counters separately expose exact and generic recur bindings, and a
defaulted-parameter test protects the generic fallback.

## Packed hot-op dispatch — 2026-09-19

Installed bytecode remains validated before execution, but the execution loop
now dispatches the common fixed-width instructions directly from
`PackedInstruction`, including pooled global loads and fixed-size lists. The
rich builder-facing `Op` is reconstructed only for the less common fallback
instructions while that transition is measured.

A 15-sample local run from the dirty worktree based on
`67071af698bb26e4cb9b7b62e024d2869a480fc0` reported:

| Workload      | Slug median | CPython median | Slug / CPython |
|---------------|------------:|---------------:|---------------:|
| function-call |   94.079 ms |      26.546 ms |          3.54x |
| n-body        |   39.863 ms |      22.158 ms |          1.80x |
| spectral-norm |   14.331 ms |      20.228 ms |          0.71x |
| binary-trees  |  107.960 ms |      29.421 ms |          3.67x |

The accompanying in-process benchmark recorded `ordinary-calls-200` at
210.427 ms for 1,000 runs, down from roughly 227 ms before the direct
hot-op path. This result also includes the preceding compact call-site
diagnostic representation, so it is a directional comparison rather than an
isolated attribution. The workload continues to preserve checked arithmetic,
collection-update metrics, and call-frame diagnostics; fallback instructions
retain the existing rich-op dispatcher.

## Type-informed numeric operator family — 2026-09-20

Semantic analysis now retains each checked expression's type for lowering.
When both operands of arithmetic or ordinary relational comparisons are proven
`num`, lowering emits private numeric bytecode. The family covers `+`, `-`,
`*`, `/`, `%`, `<`, `>`, `<=`, and `>=`. Every opcode still validates its
values at runtime because hosts may construct private bytecode directly. It
preserves integer overflow, division-by-zero, and mixed integer/float behavior.
Equality, bitwise operations, and match-guard comparisons retain their generic
opcodes because their semantics or accepted operands differ.

The in-process benchmark compares equivalent 200-step recursive sum programs.
The typed form declares both parameters as `num`; each workload executes
2,623,000 instructions over 1,000 runs.

| Workload                      |    Elapsed |
|-------------------------------|-----------:|
| arithmetic-and-branches       | 167.664 ms |
| typed-arithmetic-and-branches | 137.026 ms |

The paired source workloads give the full-process result below. The Python
implementations are identical within each pair; each typed Slug version adds
only established `num` annotations. Each value is the median of three
independent 15-sample benchmark medians.

| Workload            | Slug median | Python median | Slug / Python |
|---------------------|------------:|--------------:|--------------:|
| n-body              |   38.431 ms |     21.011 ms |         1.83x |
| typed-n-body        |   38.267 ms |     21.192 ms |         1.81x |
| spectral-norm       |   13.950 ms |     19.607 ms |         0.71x |
| typed-spectral-norm |   13.260 ms |     19.444 ms |         0.68x |

The n-body difference is within local variation, but typed spectral-norm is
about 5% faster. Keep these paired workloads as the gate for subsequent
type-informed optimizations; do not infer a general source-versus-CPython
improvement from this narrow result.

## Desktop release profile (`opt-level = 3`) — 2026-09-20

The desktop release profile now uses `opt-level = 3`; its former
size-optimized setting remains available through the separate `embedded`
profile. On revision `465d7d39bcdb2c180bbc1222f0b054cce5cc4717`, a clean
15-sample run with three warmups reported:

| Workload            | Slug median | CPython median | Slug / CPython |
|---------------------|------------:|---------------:|---------------:|
| function-call       |   44.678 ms |      25.953 ms |          1.72x |
| n-body              |   18.913 ms |      21.500 ms |          0.88x |
| typed-n-body        |   18.490 ms |      21.370 ms |          0.87x |
| spectral-norm       |    7.680 ms |      19.986 ms |          0.38x |
| typed-spectral-norm |    7.110 ms |      20.031 ms |          0.35x |
| binary-trees        |   49.839 ms |      29.049 ms |          1.72x |

Compared with the earlier size-optimized packed-dispatch result, the four
shared Slug workloads improved by roughly 46--54%. This is a release-profile
comparison, not attribution to a single VM change. The remaining source-level
targets are ordinary calls and binary-tree construction/matching; their
measurement and preservation gates are recorded in the
[VM optimization plan](../planning/vm-optimization.md).

## Packed scope entry — 2026-09-21

`EnterScope` was the dominant rich-op fallback in the source-aligned
function-call workload. The packed dispatcher now runs the same checked
scope-depth update without reconstructing the builder-facing opcode. This is a
private VM representation change; source semantics and diagnostics are
unchanged.

The in-process benchmark compiles each source workload once, then executes it
ten times through an installed program. The before and after figures below are
single local samples and show the intended runtime cost directly.

| Workload      |     Before |      After | Change |
|---------------|-----------:|-----------:|-------:|
| function-call | 498.787 ms | 477.552 ms |  -4.3% |
| binary-trees  | 548.682 ms | 532.710 ms |  -2.9% |

| Workload      | Direct / fallback before | Direct / fallback after |
|---------------|-------------------------:|------------------------:|
| function-call |   16,000,230 / 1,000,080 |         17,000,240 / 70 |
| binary-trees  |   17,694,500 / 2,293,780 |  18,349,850 / 1,638,430 |

Three independent `make bench-source` runs used three warmups and 15 timed
samples per runtime/workload. Their source-level medians remain within normal
local variation, so they do not establish an end-to-end improvement claim.

| Workload      |     Run 1 |     Run 2 |     Run 3 |
|---------------|----------:|----------:|----------:|
| function-call | 45.026 ms | 44.489 ms | 44.641 ms |
| binary-trees  | 49.890 ms | 49.130 ms | 49.406 ms |

The remaining binary-tree fallback work is approximately 98,302 `TryMatch`
instructions and 65,535 `LeaveScope` instructions per workload execution.
Evaluate those paths independently before retaining another packed fast path.
