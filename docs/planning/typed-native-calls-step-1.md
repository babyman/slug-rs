# Typed native calls, step 1: scalars, text, bytes, handles, and `slug.io.fs`
# Typed native calls, step 1: scalars, text, bytes, handles, and `slug.io.fs`

## Goal

Implement step 1 of the replacement plan in
[Typed native calls](../decisions/2026-10-10-typed-native-calls.md): typed
calls for scalar, string, bytes, and handle parameters, bound through the
clutch manifest and called through generated shims with the
[uniform entry](../decisions/2026-10-10-uniform-shim-entry.md). Prove the same
binding on a desktop dynamic library and a statically linked target, as
[static provider selection](../decisions/2026-10-11-static-typed-provider-selection.md)
requires. Then move `slug.io.fs` off the 0.13 prototype ABI.

Step 1 ends when `slug.io.fs` runs on typed calls on macOS, Linux, and
Windows, a typed fixture provider cross-builds for the ESP32-S3, and the
per-call and end-to-end measurements against the prototype are recorded.

## Out of scope

- `sqlite`, `stdin`, the `events` profile, and removing the 0.13 prototype
  (steps 2-4). `slug.core` and `slug.db.sqlite` stay on the prototype ABI
  throughout this plan, so the desktop loader supports both forms.
- Structs, `buffer` resources, out parameters other than text and status, and
  manifest constants.
- Typed Rust glue without C, and a header-reading manifest generator.
- The deferred VM work in
  [VM optimization item 10](vm-optimization.md#10-reduce-per-instruction-overhead-and-indexed-member-lookup-deferred).

## Settled inputs

These decisions are fixed for this plan:

- `foreign` declarations remain the Slug-visible contract; the manifest
  binds them to C. No source syntax changes.
- One uniform entry, one table per shim, a provider-unique table function,
  and a `slug_shim_get_table` alias only in dynamic libraries.
- Each binding carries its canonical signature text, which the loader
  compares exactly.
- Returned text and bytes are always copied, and each is borrowed or owned by
  a named free function.
- A nonzero entry status is a shim contract failure. Boundary failures use the
  closed list of codes in the typed-calls record. Library failures are
  ordinary results, and Slug wrappers throw `Error`.
- Static targets select one provider per module at build time, and one shared
  `no_std + alloc` adapter makes every shim call.

## Decisions settled for phase 1

All seven items below are settled in
[Typed native manifest bindings](../decisions/2026-10-11-typed-native-manifest-bindings.md),
following these recommendations. One refinement: a signature names the Slug
resource (`handle<File>`), and the resource entry supplies its C type, rather
than repeating the C type in each signature.

1. **Manifest syntax for typed bindings.** The final keys for functions,
   parameter types and directions, result ownership, handle C types, the
   required profile, and the C sources and headers that build the shim.
   Recommendation: keep the illustrative `[native.functions."module".name]`
   shape with `symbol` and `signature`, spell ownership inside the signature
   (`cstr owned(free)`), and name each handle's C type (`handle<FILE>`).
2. **Exact canonical signature spelling.** A grammar for the text the generator
   and loader both produce, with a fixed type spelling and separator rules.
3. **Resource close metadata.** The spike hard-coded the C function that closes
   a handle. Recommendation: the manifest declares one close binding per
   handle type. The adapter runs it from the VM's existing idempotent resource
   close and from the destructor fallback, and a Slug-visible `close` foreign
   function binds to that resource close rather than to a raw call.
4. **Where the Slug/C type compatibility check runs.** At run time the VM keeps
   only resource identities for `foreign` parameters, not their Slug types.
   Either retain declaration parameter and result types in program metadata
   for the adapter to check at registration, or check while the clutch's
   module source is compiled. Recommendation: check during compilation, where
   both the declaration and the manifest are available, and keep the runtime
   check to arity and resource identity.
5. **Maximum cell count.** Recommendation: 16 argument and 16 result cells,
   checked by the generator and the loader, so the adapter can use fixed-size
   stack arrays.
6. **Compiler flags and Windows export.** Recommendation: clang and gcc build
   with `-Wall -Wextra -Wconversion -Wsign-conversion -Werror`; the generator
   emits a `SLUG_SHIM_EXPORT` macro that expands to `__declspec(dllexport)` on
   Windows dynamic builds. MSVC is not supported until it is needed.
7. **How shims are built.** Recommendation: generate shims as part of clutch
   staging and static builds, never commit generated C, and run the
   generator's check mode in CI against a temporary output. Whether static
   builds use the `cc` crate (a new build dependency) or invoke the compiler
   directly is decided in phase 5.

## Phases

Each phase lands as its own commits with focused tests, and leaves
`make check` and the three-platform `ffi-prototype` job green.

### 1. Manifest model

A crate named `slug-native-manifest` (name not final) that depends on neither
`slug-vm` nor `slug-frontend`.

- [x] Settle decisions 1-7 above in the decision records.
- [x] Parse the typed manifest section into a model of functions, parameters,
  directions, result ownership, handle types, close bindings, and profile.
  Malformed input is a checked error naming the manifest path of the module
  and binding; the caller adds the clutch.
- [x] Produce the canonical signature text from the model, and test that
  formatting, comments, and parameter names do not change it.
- [x] Reject bindings above the maximum cell count and unknown types. TOML
  table keys make duplicate bindings unrepresentable.

### 2. Shim generator

Rust code in the manifest crate that emits one deterministic C translation
unit.

- [x] Emit `slug_shim_`-prefixed internal-linkage entries, exact-type header
  checks for each target and free function, release functions for owned
  results, `strlen` lengths for `cstr` results, and the table with signatures.
- [x] Emit the provider-unique table function, plus the exported alias for
  dynamic builds.
- [x] Add a check mode that regenerates and compares. It compares in memory
  and names the first differing line, so it needs no temporary file.
- [x] Port the spike's width, qualifier, and arity mismatch tests to generated
  output, and add a mismatch test for a free function.
- [x] Test that a generated shim for a function named `write` compiles in a
  translation unit that includes the platform's C library headers.

### 3. Shared adapter

A `no_std + alloc` crate that depends on `slug-vm` and is the only code that
calls a shim entry.

- [x] Encode arguments into cells by manifest type with range checks,
  `cstr` copies, and resource type and closed-state checks.
- [x] Decode results with UTF-8 validation and NULL-to-`nil` mapping. Run
  cleanup after decoding, including on failure.
- [x] Map entry status and boundary failures to the closed list of codes.
- [x] Verify each binding's signature, argument count, and result count at
  installation.
- [x] Confine `unsafe` to one private module: the entry and release calls,
  plus reading the shim table and copying returned text, which the adapter
  cannot do safely. Keep `unsafe_code = "deny"` in `slug-vm` and
  `slug-frontend`.
- [x] Unit-test encoding, decoding, and failure mapping against Rust test
  entries, without a C compiler.

The adapter takes each binding's cell layout from its canonical signature
text, so static images carry no manifest model. CI checks it for the
ESP32-S3 target in the `embedded-check` job.

### 4. Desktop dynamic path

- [x] Accept a typed native section in `clutch.toml` alongside the existing
  prototype `abi` form, and dispatch activation on which form is present.
- [x] Load the library, resolve `slug_shim_get_table`, check the ABI version,
  and register each binding through the adapter.
- [x] Implement the Slug/C type compatibility check where decision 4 places it,
  including `native.type` for a NULL `cstr` result annotated `str` without
  `nil`, which the adapter cannot see.
- [x] Extend clutch staging to generate and compile shims for typed clutches.
- [x] Port the spike fixture's functional and checked-boundary tests to the
  production path, and keep them in the three-platform job.

The check runs through a `ModuleGraphHost` hook after each clutch module
compiles and before its activation registers, so a second module from an
already-active clutch is checked too. The frontend passes each `foreign`
declaration's annotations as text. The adapter's `install` returns a switch
per binding that the check sets when a text result excludes `nil`.

Staging is `slug_native_manifest::build_dynamic`, also available as
`slug-native-shim stage`. `scripts/stage-native-clutches.sh` calls it once
`slug.io.fs` moves in phase 6, because no first-party clutch is typed before
then. The production tests use a fixture with plain fixed-width C types
(`crates/slug/tests/typed_native/`), since the spike's enum status type would
not match an `i32` binding exactly.

### 5. Static path

- [x] Add `kind` to target-descriptor provider entries, defaulting to `rust`.
- [x] For a `typed` provider, generate the shim and Rust registration at build
  time, compile with the target C compiler, and link.
- [x] Reject missing coverage, type incompatibility, header mismatch, and an
  unsupported profile before linking.
- [x] A desktop test statically links two generated shims into one binary.
- [x] Add a small typed C fixture provider to an ESP32-S3 descriptor and keep
  it in the cross-build gate. The PinPulse providers stay in Rust.

Static builds invoke the target C compiler and archiver directly through
`slug_embedded_build::CToolchain`, with no `cc` build dependency, matching
`build_dynamic`. A typed entry names its `clutch`; `[firmware] profiles`
defaults to `["core"]`. One shim is built per clutch, named from the clutch
directory, and every module the clutch binds must be selected from it. The
Slug/C check moved to `slug_native_manifest::check_foreign_declarations` so the
desktop loader and the build share it. The two-shim test is the
`slug-typed-static-test` crate, whose `build.rs` acts as a board package. The
ESP32-S3 fixture is `fixture.pulse`, selected by `esp32-s3-typed.toml`; a
release build links it and was verified to contain its shim table, but it has
not been flashed.

### 6. Move `slug.io.fs`

- [ ] Rewrite `fs.c` as plain exported functions with a header, and replace
  the prototype manifest section with typed bindings.
- [ ] Rewrite `fs.slug` as private raw foreign declarations plus exported Slug
  wrappers that keep every public name, signature, and documented behavior.
  Failures are thrown as `Error` values with the existing `native.io` and
  `native.alloc` types.
- [ ] Keep the 16 MiB line limit and idempotent close. A read after close still
  reports `native resource is closed`.
- [ ] Pass the existing filesystem CLI tests unchanged, and add tests for the
  thrown error values and their `type` fields.
- [ ] Record the accepted diagnostic change for uncaught `fs` failures in the
  changelog.

### 7. Measure and clean up

- [ ] Measure scalar per-call cost through the production path against the
  0.13 baseline with `make bench-ffi-prototype-scalar`, extended with the new
  path.
- [ ] Measure end-to-end behavior on the existing filesystem fixtures, typed
  versus prototype, and record both in a results document. No CI timing
  thresholds.
- [ ] Delete the bounded Rust dispatcher, its shape table, and the spike-only
  shim fixtures, which the uniform shim entry record retires when the loader
  lands.
- [ ] Update `docs/reference/native-abi.md`,
  `docs/reference/experimental-clutches.md`, and the README clutch paragraph
  for typed bindings.

## Exit gate

- `slug.io.fs` runs on typed calls, and its CLI tests pass on macOS, Linux, and
  Windows.
- A typed fixture provider passes the ESP32-S3 cross-build gate.
- The generator's check mode, header-mismatch tests, and the
  two-shims-in-one-binary test pass in CI.
- `slug-vm` and `slug-frontend` still deny `unsafe`, and no host panic is
  reachable from a typed call.
- Measurements against the prototype are recorded, and the spike-only
  dispatcher is deleted.

## Verification

- `make check` and `make test-ffi-prototype` for each phase.
- `cargo +esp check` in `firmware/slug-esp32-s3` for phases 3 and 5.
- `git diff --check` and `make docs-check` for decision and documentation
  changes.
