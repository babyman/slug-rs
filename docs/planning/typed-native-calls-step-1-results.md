# Typed native calls, step 1: measurements

Measurements for phase 7 of
[Typed native calls, step 1](typed-native-calls-step-1.md): one scalar call
through the production typed path, and the `slug.io.fs` clutch end to end,
each against the 0.13 prototype. These are local evidence, not CI thresholds.

## How they were taken

`make bench-ffi-prototype-scalar` runs the ignored benchmarks in
`crates/slug/tests/ffi_prototype.rs` and `crates/slug/tests/typed_native.rs`
in release mode. It saves the command, compiler and Rust versions, host, and
raw samples to the ignored `docs/planning/local/ffi-prototype-scalar-baseline.txt`.

This run used an Apple M3 on macOS (Darwin 25.6.0), Rust 1.96.1, and Apple
Clang 21.0.0. Each scalar series is 11 samples of 100,000 calls after a
10,000-call warm-up. The figures are medians.

The run was taken before the spike-only dispatcher and fixtures were deleted,
so it also measures those candidates on the same host for comparison. After
the deletion, the command reports the 0.13 baseline, the VM controls, the
production typed path, and the filesystem comparison.

## Scalar call

Every path runs the same Slug loop, `scalar.add(remaining, 1)` followed by
`recur`. The empty loop is that loop with the call removed.

| Path | Median ns/call | Minus empty loop |
|---|---:|---:|
| Empty loop | 140 | 0 |
| Statically registered Rust native | 297 | 157 |
| Ordinary Slug function | 321 | 181 |
| Spike: uniform generated shim | 322 | 182 |
| Spike: bounded Rust dispatcher | 326 | 186 |
| Spike: original generated shim | 327 | 187 |
| 0.13 host-table callback | 339 | 199 |
| **Production typed path** | **371** | **231** |

The production typed path is a staged clutch library loaded by the desktop
loader and called through `slug-native-adapter`, the path `slug.io.fs` now
uses. Net of the loop, it costs about 32 ns (16%) more per call than the 0.13
callback, about 49 ns more than the spike's uniform shim, and about 74 ns more
than a Rust native.

The spike's uniform shim used the same C entry and cells, so the gap is in the
adapter's per-call Rust work. These likely causes are not yet profiled:

- each call allocates two vectors, one for decoded cells and one for the
  result values, even for a single scalar result;
- each call parses its binding index from the member-key string and clones
  two `Rc`s from module state; and
- the production path adds checks the spike skipped: range checks, a
  contract status check, and the NULL-text switch.

A follow-up should profile this path before changing it. The step 1 plan sets
no performance target for the typed path.

## Filesystem end to end

The same program runs through each `slug.io.fs` clutch in one process. It writes
10,000 lines one call at a time, each write flushed, then reads them back.
The prototype series uses the unchanged 0.13 copy in
`crates/slug/tests/ffi_prototype/`. The typed series uses the current clutch,
built as clutch staging builds it.

| Clutch | Median ns per write or read | Range |
|---|---:|---:|
| 0.13 prototype | 1,577 | 1,536–1,675 |
| Typed bindings | 1,900 | 1,873–1,980 |

The typed clutch costs about 323 ns (20%) more per operation. Besides the
adapter cost above, each public function is now a Slug wrapper. A wrapper adds
a Slug call, and `readLine` builds and matches a two-element result list.
`write` flushes on every call in both clutches, so both figures include a
`fflush` system call.

Behavior is the same on every success path. The non-ignored
`typed_filesystem_matches_the_prototype_end_to_end` test runs one CLI program
through both clutches. It requires identical output, exit status,
closed-handle diagnostic, and written file. The intended differences are
failures thrown as `Error` values, and a line containing a NUL byte now
failing. Both are recorded in the changelog.
