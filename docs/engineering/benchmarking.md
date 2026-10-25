# Benchmarking

Slug maintains two complementary benchmark layers. Neither is a
timing-sensitive CI assertion; compare repeated local runs rather than
recording portable performance claims.

`make bench-vm` runs `crates/slug-frontend/benches/vm.rs`. It executes representative
source programs in-process and reports opt-in VM counters such as instruction
dispatch, frame-local vector creation and capacity, argument-to-local movement,
collection operations, and executable layout. Use it to identify the
implementation cost an internal representation change affects.

The same counters are available for any program, including the real-program
workloads that import modules or clutches and so cannot run in-process. A
`slug` executable built with the `metrics` feature prints them to stderr after
a program finishes when `SLUG_DUMP_METRICS` is set:

```sh
cargo build --release -p slug --bin slug --features metrics --target-dir target/metrics
SLUG_DUMP_METRICS=1 target/metrics/release/slug program.slug
```

A separate target directory keeps the counting build from replacing the
release executable the timing runners use.

`make bench-source` builds the profile-guided `slug` executable (see
[Profile-guided builds](#profile-guided-builds)) and runs
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
- `closure-accumulate` appends to an accumulator and then creates closures
  in the same loop step, so the append must still extend the list in place.
- `fib` is naive non-tail recursion: ordinary calls and returns with no
  `recur`.
- `defer-errors` registers scoped cleanup on every call and recovers a thrown
  error from one call in eight, so fast returns do not apply.
- `word-frequency` counts string keys in a persistent map and reads them back.
- `string-building` builds a report with interpolation and concatenation.
- `expression-evaluator` tokenizes, parses, constant-folds, and repeatedly
  evaluates arithmetic expressions over structs and maps: a small mixed
  program rather than a single hot loop.
- `graph-search` runs a breadth-first search over an integer-keyed adjacency
  map, growing visited and distance maps as it goes.
- `text-wrap` wraps a generated paragraph to a fixed width with indexing,
  interpolation, and concatenation.

A third group is real programs, copied from the agent evaluation projects'
reference solutions, or from an agent's solution, so that edits there do not
change them. They have no CPython twin, so `make bench-source` reports only
their Slug time:

- `invaders-game` plays the Space Invaders reference for 7,300 frames with
  the `bot-long-1` case's recorded inputs, calling `game.tick` directly
  without the slug.arcade clutch.
- `crawl-game` plays the first 30 commands of the Crawl `bot-106` case in an
  agent's solution, which keeps its game state in maps and reads it by field
  (`g.player.x`), rendering the screen after each command without the
  slug.arcade clutch.
- `http-server` serves the HTTP reference's routes to four keep-alive clients
  in the same program, 4,228 requests over loopback. It needs the staged
  `slug.net.tcp` clutch (`make stage-native-clutches`); both runners set
  `SLUG_HOME` to the checkout when it is unset.
- `tiny-interpreter` runs a small program with recursion, closures, lists,
  and strings in the Tiny interpreter reference.

`expression-evaluator`, `graph-search`, `text-wrap`, and `tiny-interpreter` are a hold-out set, marked `held_out` in
`crates/slug-frontend/benches/source.rs` and in both reports. Check them for
regressions, but never use them to choose or tune an optimization: they stay
useful only while nothing has been fitted to them. When a hold-out workload
does drive a change, move it to the tuned group and add a fresh hold-out.
`word-frequency` started as a hold-out. It moved to the tuned group when its
profile motivated the indexed-map work, and `graph-search` replaced it.
`string-building` likewise moved when it motivated in-place string append,
and `text-wrap` replaced it.

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

## Profile-guided builds

Release executables are profile-guided builds, and the benchmark runners
measure them. `make release-pgo` builds one at `target/pgo/slug`. It
needs the toolchain's `llvm-profdata`, from `rustup component add
llvm-tools`. `scripts/pgo-build.py` builds an instrumented executable, runs
it once over every tuned workload, merges the profiles, and rebuilds with
them. It never trains on the hold-out workloads, so they show whether a
profile generalizes. It passes `--target` so build scripts are not
instrumented, and keeps its builds under `target/pgo/`, apart from the
ordinary release build.

On 2026-10-25 (`ee62cc3`) the profile-guided build ran every workload
faster than the ordinary release build of the same revision. The tuned
geomean was 0.86x and the hold-out geomean 0.85x. The real programs ran
0.83-0.86x and `shared-append` 0.52x.

A profile-guided build also chooses code layout from measured behavior, and
the dispatch loop's layout has swung single changes by several percent
(see `docs/planning/vm-optimization.md`). `make bench-ab` therefore compares
profile-guided builds on both sides. Each side trains on this checkout's
workloads, so both learn from the same programs. A revision's build is
cached under its commit and a hash of the training programs. Building one
takes about a minute and a half. `--no-pgo` compares ordinary release
builds:

```sh
make bench-ab BENCH_AB_ARGS="--no-pgo"
```

The in-process suite also includes a nested task tree. Unlike the timer and
large-`select` pressure cases, it measures structured task fanout, nested
nursery settlement, and task-await resumption together. It is a scheduler
coverage workload, not an asynchronous I/O throughput claim.

`fannkuch-redux-7` in the in-process suite runs the same seven-value program
and checks its final checksum, while exposing its list-construction, update,
and ownership counters. Use those counters to evaluate persistent-list changes
before interpreting end-to-end timing changes.
