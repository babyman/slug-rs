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
