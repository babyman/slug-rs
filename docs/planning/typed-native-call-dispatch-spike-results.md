# Typed native-call dispatch spike results

The private fixture compares a generated C shim with a bounded Rust dispatcher
for the first filesystem-shaped typed native calls. It does not define a
manifest schema, public C ABI, error mapping, or result-text ownership rule.

## Local measurement

Run `make bench-ffi-prototype-scalar`. It captures the command, compiler and
Rust versions, target, operating-system details, sample count, and raw output
in the ignored `docs/planning/local/ffi-prototype-scalar-baseline.txt`.

The initial macOS ARM64 run used Rust 1.96.1, Apple Clang 21.0.0, release mode,
11 samples, a 10,000-call warm-up, and 100,000 calls per sample. Its medians
were 731 ns/call for the 0.13 host-table callback baseline, 690 ns/call for
the Rust dispatcher, and 679 ns/call for the generated shim. These are local
evidence, not a CI threshold; the raw output is the authoritative record.

## Comparison

| Criterion | Generated shim | Rust dispatcher |
|---|---|---|
| Header/signature mismatch | The adapter includes the fixture header. A deliberately incompatible function-pointer declaration fails with `-Werror`. | Not checked by the platform loader. A declaration differing from the selected trampoline is undefined behavior. |
| Dynamic binding build tools | Requires a C compiler to generate and compile the adapter with the library or clutch source. | Resolves the shared-library symbol without compiling an adapter. |
| Fixture shapes | All six filesystem bindings and scalar `add` pass against the direct fixture. | The same bindings pass through five finite trampolines. |
| Checked boundaries | Interior NUL, closed resource, end-of-file, native I/O failure, missing symbol, and unsupported shape are covered. | The same checked boundaries are covered. |
| Platform coverage | The focused tests are part of `make test-ffi-prototype`, which the existing macOS, Linux, and Windows CI matrix runs. | The same matrix invocation covers this candidate. |
| Scalar cost | No local regression versus 0.13; adapter overhead was not measurable in this run. | No local regression versus 0.13. |
| Unsafe boundary | Only the desktop loader performs dynamic loading and calls. | Only the desktop loader performs dynamic loading and calls. |

The fixture and generated adapter add small private C files and one generator
script. A library author supplies the declaration-owning header and invokes
the generator during a clutch or static-host build. A static host compiles the
same generated adapter with its native objects. Rust dispatch avoids that
build step but retains the unverifiable wrong-signature risk.

## Outcome

Generated shims are the preferred first implementation for header-backed
typed bindings. The bounded Rust dispatcher remains useful as an experimental
comparison and as evidence that the selected shapes do not need `libffi`, but
it is not the production choice because a manifest/header mismatch can become
undefined behavior before any checked Slug boundary is reached.

The existing three-platform CI job remains the functional platform gate for
these focused tests. No timing assertion is added to CI.
