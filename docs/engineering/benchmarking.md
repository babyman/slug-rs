# Benchmarking

Slug maintains two complementary benchmark layers. Neither is a
timing-sensitive CI assertion; compare repeated local runs rather than
recording portable performance claims.

`make bench-vm` runs `crates/slug-frontend/benches/vm.rs`. It executes representative
source programs in-process and reports opt-in VM counters such as instruction
dispatch, frame-local vector creation and capacity, argument-to-local movement,
collection operations, and executable layout. Use it to identify the
implementation cost an internal representation change affects.

`make bench-source` builds the release `slug` executable and runs
`crates/slug-frontend/benches/source.rs`. The runner starts a fresh Slug or CPython
process for each sample, so its timings include source compilation, VM startup,
and program execution. It validates each checked-in pair of implementations
against its expected output before timing it. It reports the median and the
`Slug / Python` ratio, where values below `1.00x` mean Slug was faster.

The default CPython reference is `/usr/bin/python3`. Its resolved path and
version, the Slug executable/version, the current Git revision, and whether
tracked local changes were present appear in the report. Override either runtime
only for an explicitly labelled comparison:

```sh
SLUG_BENCH_PYTHON=/path/to/python3 make bench-source
SLUG_BENCH_SLUG=/path/to/slug make bench-source
SLUG_BENCH_SAMPLES=25 make bench-source
cargo bench -p slug-frontend --bench source -- --json
```

The initial workload corpus is Benchmark-Game-inspired, rather than a port of
the upstream corpus: function calls, untyped and `num`-annotated pairs of
small n-body-style floating-point loops and spectral-norm-style nested numeric
loops, binary trees, and a single-threaded fannkuch-redux port. Fannkuch-redux
adds permutation generation, indexed list reads, prefix reversal, and
persistent list construction; it is not a claim of Benchmarks Game result
comparability. It evaluates all 5,040 permutations of seven values, enough to
make list transformation materially outweigh fresh-process startup. Each pair
is intentionally written in the idioms currently
supported by Slug and Python; keep output checks and workload intent aligned
when adding a pair.

A second group widens the corpus beyond tight numeric `recur` loops, so that
an optimization tuned on one shape cannot pass unnoticed when it slows
another:

- `shared-append` appends to a list while the previous version stays live,
  so `:+` must copy rather than extend in place.
- `higher-order` runs `slug.std` `map`, `filter`, and `reduce` with closures
  that capture their caller's locals.
- `fib` is naive non-tail recursion: ordinary calls and returns with no
  `recur`.
- `defer-errors` registers scoped cleanup on every call and recovers a thrown
  error from one call in eight, so fast returns do not apply.
- `word-frequency` counts string keys in a persistent map and reads them back.
- `string-building` builds a report with interpolation and concatenation.
- `expression-evaluator` tokenizes, parses, constant-folds, and repeatedly
  evaluates arithmetic expressions over structs and maps: a small mixed
  program rather than a single hot loop.

The last three are a hold-out set, marked `held_out` in
`crates/slug-frontend/benches/source.rs` and in both reports. Check them for
regressions, but never use them to choose or tune an optimization: they stay
useful only while nothing has been fitted to them. When a hold-out workload
does drive a change, move it to the tuned group and add a fresh hold-out.

## Interleaved A/B comparison

`make bench-ab` compares two builds of `slug` over the source corpus and
decides whether a change is worth keeping. Sequential runs drifted by up to 30% within
one session on a laptop, which is larger than most single optimizations, so
the runner alternates the two binaries inside every round and alternates
which side goes first:

```sh
make bench-ab                                  # master vs. working tree
make bench-ab BASE=6d48f4a                     # another base revision
make bench-ab BASE=master CANDIDATE=feature/vm # two revisions
make bench-ab BENCH_AB_ARGS="--rounds 15 --only fib,binary-trees"
python3 -I scripts/bench-ab.py --bin old=/path/a --bin new=/path/b --json
```

Each revision is built once in a temporary detached worktree and cached under
`target/bench-ab/`; the working tree is rebuilt on every run. Both builds
must print identical output for every workload before timing starts. The
report gives each side's median wall-clock time, the candidate/base ratio,
each side's interquartile range relative to its median, and peak RSS, then
the geometric-mean ratio of the tuned and hold-out groups separately. Treat a
ratio inside the spread as noise, and read the hold-out geomean before
accepting a change.

The in-process suite also includes a nested task tree. Unlike the timer and
large-`select` pressure cases, it measures structured task fanout, nested
nursery settlement, and task-await resumption together. It is a scheduler
coverage workload, not an asynchronous I/O throughput claim.

`fannkuch-redux-7` in the in-process suite runs the same seven-value program
and checks its final checksum, while exposing its list-construction, update,
and ownership counters. Use those counters to evaluate persistent-list changes
before interpreting end-to-end timing changes.
