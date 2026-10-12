# Typed native-call dispatch spike results

The spike's dispatcher, shims, and fixtures were deleted when the production
typed path landed. See [Step 1 measurements](typed-native-calls-step-1-results.md)
for the production path, measured beside the spike candidates on the same host.

The private fixture compares a generated C shim with a bounded Rust dispatcher
for the first filesystem-shaped typed native calls. It does not define a
manifest schema, public C ABI, error mapping, or result-text ownership rule.

## Local measurement

Run `make bench-native-scalar`. It captures the command, compiler and
Rust versions, target, operating-system details, sample count, and raw output
in the ignored `docs/planning/local/native-scalar.txt`.

The latest macOS ARM64 run used Rust 1.96.1, Apple Clang 21.0.0, release mode,
11 samples, a 10,000-call warm-up, and 100,000 calls per sample. Its medians
were 721 ns/call for the 0.13 host-table callback baseline, 698 ns/call for
the bounded Rust dispatcher, 692 ns/call for the original generated shim, and
710 ns/call for the uniform generated shim. The controls measured 395 ns/call
for an ordinary Slug function and 673 ns/call for a statically registered Rust
native function.

An empty-loop control runs the same `repeat` recursion with the call removed,
so the loop's own compare, subtract, and `recur` cost can be subtracted from
each control. A later run on the same host measured it at 169 ns/call. That run
was faster overall than the one above, so the loop-subtracted figures use only
its own medians:

| Path | Median ns/call | Minus empty loop |
|---|---:|---:|
| Empty loop | 169 | 0 |
| Ordinary Slug function | 379 | 210 |
| Statically registered Rust native | 605 | 436 |
| Bounded Rust dispatcher | 610 | 441 |
| Original generated shim | 610 | 441 |
| Uniform generated shim | 618 | 449 |
| 0.13 host-table callback | 637 | 468 |

A native call through the VM therefore costs about 226 ns more than an
ordinary Slug function call, before any dispatch candidate is involved. The
empty loop omits the discarded call result, so it slightly understates the
loop share of each control.

### After the native-call path fixes

A sampling profile of the static Rust native control showed that the native
callback and `invoke_native` together took about 17 ns per call. The rest of
the gap was VM call setup: each call deep-copied the declared native's
identity and resource signature several times, rebuilt its arguments through
closure-only binding, cloned values it had just popped, and allocated an
unused overflow message for each integer operation. Four VM refactors removed
those costs without changing checked behavior. The same benchmark on the same
host then measured:

| Path | Median ns/call | Minus empty loop | Before fixes, minus loop |
|---|---:|---:|---:|
| Empty loop | 141 | 0 | 0 |
| Ordinary Slug function | 331 | 190 | 210 |
| Statically registered Rust native | 307 | 166 | 436 |
| Bounded Rust dispatcher | 328 | 187 | 441 |
| Original generated shim | 325 | 184 | 441 |
| Uniform generated shim | 342 | 201 | 449 |
| 0.13 host-table callback | 353 | 211 | 468 |

A native call now costs slightly less than an ordinary Slug function call, and
the dispatch candidates remain within noise of each other. About 80 ns of each
remaining native call is the `scalar.add` member read, a linear scan of the
imported module's map that grows with module size. Resolving import members
before execution is the next candidate for per-frame native calls.

The plain-C loop made 100 million direct and uniform-shim calls against the
same fixture. It measured 1.05 ns/call direct and 1.31 ns/call through the
uniform shim. This is a small measurable wrapper cost, but it is much smaller
than the VM/native-call path and is not a CI threshold. The raw output is the
authoritative record for all timings.

## Comparison

| Criterion | Generated shim | Rust dispatcher |
|---|---|---|
| Header/signature mismatch | The adapter includes the fixture header. A deliberately incompatible function-pointer declaration fails with `-Werror`. | Not checked by the platform loader. A declaration differing from the selected trampoline is undefined behavior. |
| Dynamic binding build tools | Requires a C compiler to generate and compile the adapter with the library or clutch source. | Resolves the shared-library symbol without compiling an adapter. |
| Fixture shapes | All six filesystem bindings and scalar `add` pass against the direct fixture. | The same bindings pass through five finite trampolines. |
| Checked boundaries | Interior NUL, closed resource, end-of-file, native I/O failure, missing symbol, and unsupported shape are covered. | The same checked boundaries are covered. |
| Platform coverage | The focused tests are part of `make test-native`, which the existing macOS, Linux, and Windows CI matrix runs. | The same matrix invocation covers this candidate. |
| Scalar cost | The original and uniform entries measured 692 and 710 ns/call respectively, versus 721 ns/call for 0.13. The uniform wrapper alone measured 1.31 ns/call versus 1.05 ns/call direct. | 698 ns/call; no local regression versus 0.13. |
| Unsafe boundary | Only the desktop loader performs dynamic loading and calls. | Only the desktop loader performs dynamic loading and calls. |

The fixture and generated adapter add small private C files and one generator.
A library author supplies the declaration-owning header and invokes the
generator during a clutch or static-host build. A static host compiles the same
generated adapter with its native objects. Rust dispatch avoids that build step
but retains the unverifiable wrong-signature risk.

The uniform-entry follow-up also passes its local fixture and rejects deliberate
integer-width, pointer-qualifier, and arity mismatches with `-Werror`. The VM
controls showed that the VM's native-call path, rather than the shim wrapper,
dominated the measured cost; the native-call path fixes above reduced it.

## Outcome

Generated shims are the preferred first implementation for header-backed
typed bindings. The bounded Rust dispatcher remains useful as an experimental
comparison and as evidence that the selected shapes do not need `libffi`, but
it is not the production choice because a manifest/header mismatch can become
undefined behavior before any checked Slug boundary is reached.

The existing three-platform CI job is the functional platform gate for these
focused tests, including the uniform mismatch tests. It passed on macOS, Linux,
and Windows at `f0a3883`, and the
[spike plan](typed-native-call-dispatch-spike.md) is complete. No
timing assertion is added to CI.
