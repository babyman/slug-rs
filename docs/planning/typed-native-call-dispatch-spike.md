# Typed native-call dispatch spike

## Goal

Choose the implementation strategy for the first typed native-call path before
committing to a manifest schema or replacing the 0.13 prototype ABI. The spike
compares generated C shims with a bounded Rust shape dispatcher for the small
set of call shapes needed by `slug.io.fs`.

This is an investigation plan, not a native-interface decision record. Its
output is evidence for a follow-up ADR and must not establish a new public C
ABI, source syntax, or manifest format.

## Questions to answer

1. Can generated shims include an upstream or clutch-owned C header so a
   manifest/header signature mismatch fails at C compile time?
2. Can a Rust shape dispatcher call the same dynamic-library symbols on every
   supported desktop platform without a C compiler or `libffi`?
3. What does each option require from a clutch author and from a static host?
4. What is the steady-state cost of a scalar typed call relative to the 0.13
   host-table callback path?
5. Can both approaches preserve checked failure boundaries and retain the
   existing macOS, Linux, and Windows dynamic-loader test coverage?

The spike does not decide error-code mapping, resource destruction metadata,
or the final ownership notation for returned strings. It identifies whether
either dispatch approach prevents those later rules from being implemented.

## Scope

Use a small, private direct C fixture shaped after `slug.io.fs`. It must not
reuse the prototype callback ABI. Its callable surface is limited to these
representative shapes:

| Operation | C shape | Why it is included |
|---|---|---|
| `openRead` | `(cstr) -> handle` | NUL-terminated path conversion and resource return |
| `openWrite` / `openAppend` | `(cstr) -> handle` | Confirms several bindings can share one shape |
| `readLine` | `(handle, out text) -> status` | Handle input plus nullable/owned text result without prematurely choosing `cstr` ownership rules |
| `write` | `(handle, str) -> i64` | Borrowed pointer-and-length text input and scalar result |
| `close` | `(handle) -> status` | Resource input and status-only return |
| `add` benchmark fixture | `(i64, i64) -> i64` | I/O-free scalar-call measurement |

The fixture header must declare each direct function. The generated-shim
candidate includes that header; an intentionally incompatible declaration in
the generated shim is a required negative compilation test. The fixture may
use opaque C handles and a simple status enum, but it must not expose a VM
value, a VM allocation, or a callback into Slug.

Do not add structs by value, variadics, lists, maps, buffers, events, threads,
or callbacks. Do not migrate the shipped filesystem clutch as part of this
spike.

## Candidate A: generated C shim

Generate a clutch-local C translation unit from a private binding description.
It includes the fixture's real header and exports a fixed Slug-facing adapter
for every selected binding. The adapter may normalize the direct function's
status and out parameters into a representation the experimental Rust loader
can consume, but it must make exactly one direct call to the target function.

Record:

- required build tools and generated artifacts for dynamic and static links;
- the generated compile command and platform-specific linker inputs;
- the diagnostic produced by a deliberately wrong parameter or result type;
- whether the same generator can support a normal installed system library
  without handwritten C; and
- whether adapter code changes the measured steady-state call cost.

The important safety property is compile-time checking against the included
header. A C shim only improves the situation when the header is the authority
for the target function declaration, rather than a duplicate handwritten
prototype.

## Candidate B: bounded Rust shape dispatcher

Implement only the table-driven dispatch needed by the fixture shapes above.
The desktop loader resolves a symbol and selects a Rust trampoline based on the
private binding description. Keep every `unsafe` operation in the loader; the
VM and frontend must remain free of `unsafe`.

Record:

- the exact supported shape grammar, including arity and out-parameter limits;
- how a resolved symbol is converted to a callable function pointer on each
  supported platform;
- the diagnostic produced for an unsupported shape;
- which mismatches remain undefined behavior because they cannot be verified
  against a C header; and
- generated code size or compile-time cost if the dispatcher is expanded by
  macros.

The candidate is acceptable only if all chosen shapes work without a C build
step. It is not a partial implementation of arbitrary C calls.

## Method

### 1. Establish the prototype baseline

- [x] Preserve the current `make test-ffi-prototype` suite and its GitHub Actions
  matrix (`macos-latest`, `ubuntu-latest`, and `windows-latest`).
- [x] Add an isolated 0.13 scalar fixture matching the `add` operation. It
  performs no I/O, allocation, formatting, source compilation, or process
  startup inside the timed loop.
- [x] Capture the command, compiler and Rust versions, target triple,
  optimization mode, CPU details where available, sample count, and raw timing
  output. Run `make bench-ffi-prototype-scalar`; it writes the ignored local
  record at `docs/planning/local/ffi-prototype-scalar-baseline.txt`.

### 2. Build equivalent private fixtures

- Give both candidates the same direct C library and the same Slug-visible
  fixture module contract.
- Exercise successful calls and checked boundary failures: invalid interior-NUL
  path, a closed handle, read end-of-file, a native I/O status, missing symbol,
  and unsupported shape.
- Verify the shim's deliberately mismatched generated declaration is rejected
  by the C compiler. The Rust-dispatch result for that case must be documented
  as a load-time limitation or undefined-behavior risk, not hidden by a test.

### 3. Measure and compare

- Benchmark only the scalar `add` call in a release build after warm-up; report
  median and distribution, not a CI timing threshold.
- Measure the 0.13 callback baseline and both candidate paths in the same
  environment and with identical loop counts.
- Separately record non-timing results: added lines and generated artifacts,
  required tools, supported platforms, source/library author workflow, and
  failure diagnostics.
- Repeat functional tests in the existing three-platform CI matrix. Timing
  comparisons are local evidence only and must not become timing-sensitive CI
  assertions.

## Decision criteria

The follow-up ADR must select a direction only if the spike can answer the
following table with reproducible evidence.

| Criterion | Generated shim | Rust dispatcher |
|---|---|---|
| Header/signature mismatch detected before runtime | Required for selected header-backed bindings | Not expected; remaining risk must be explicit |
| C compiler required for dynamic-library binding | Yes | No |
| First-slice filesystem shapes work | Required | Required |
| macOS, Linux, and Windows functional coverage | Required | Required |
| VM/frontend remain `unsafe`-free | Required | Required |
| Scalar cost versus 0.13 | Measured, no preset threshold | Measured, no preset threshold |
| Additional dependency | None beyond platform C toolchain | None; otherwise stop and reassess `libffi` separately |

Prefer generated shims if their header checking works with an acceptable
clutch build workflow and their scalar cost is not materially worse than the
dispatcher. Prefer the bounded dispatcher only if its no-build-step benefit
outweighs its explicitly retained wrong-signature risk. If neither is viable,
write a separate dependency evaluation for `libffi`; do not add it during the
spike.

## Deliverables and exit gate

1. Two private, removable prototype implementations and focused tests.
2. A checked-in benchmark command or script plus an ignored/local result
   record containing the measured environment and raw numbers.
3. A concise comparison report covering every decision criterion, including
   failed or unsupported cases.
4. A follow-up ADR selecting the dispatch strategy, or an explicit conclusion
   that neither candidate is ready.

The spike is complete when both candidates have either passed the fixture and
platform gates or have a documented concrete blocker. Production manifest
parsing, profile enforcement, error mapping, filesystem migration, buffer
resources, SQLite, and events begin only after that outcome is reviewed.

## Verification

- `make test-ffi-prototype` for the existing prototype compatibility baseline.
- Focused tests for each private candidate and its negative cases.
- Existing three-platform FFI CI matrix for functional behavior.
- `git diff --check` and `make docs-check` before updating this planning
  document with results.
