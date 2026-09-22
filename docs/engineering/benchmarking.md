# Benchmarking

Slug maintains two complementary benchmark layers. Neither is a
timing-sensitive CI assertion; compare repeated local runs rather than
recording portable performance claims.

`make bench-vm` runs `crates/slug-vm/benches/vm.rs`. It executes representative
source programs in-process and reports opt-in VM counters such as instruction
dispatch, frame-local vector creation and capacity, argument-to-local movement,
collection operations, and executable layout. Use it to identify the
implementation cost an internal representation change affects.

`make bench-source` builds the release `slug` executable and runs
`crates/slug-vm/benches/source.rs`. The runner starts a fresh Slug or CPython
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
cargo bench -p slug-vm --bench source -- --json
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

The in-process suite also includes a nested task tree. Unlike the timer and
large-`select` pressure cases, it measures structured task fanout, nested
nursery settlement, and task-await resumption together. It is a scheduler
coverage workload, not an asynchronous I/O throughput claim.
