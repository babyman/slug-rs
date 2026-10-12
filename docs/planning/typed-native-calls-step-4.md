# Typed native calls, step 4: remove the 0.13 prototype

## Goal

Implement step 4 of the replacement plan in
[Typed native calls](../decisions/2026-10-10-typed-native-calls.md): remove
the prototype loader, its tests, the headers, and the helper SDK. Since
step 3, no first-party clutch uses the prototype, so nothing that ships
depends on it.

Step 4 ends when the repository has one native form, typed bindings, and
nothing runtime-visible names the prototype ABI. Removal must not lose
coverage of loader behavior that typed clutches rely on, and the CI jobs must
pass on macOS, Linux, Windows, and ESP32-S3.

## Out of scope

- Changes to `slug-vm`. The native extension interface (`NativeCall`,
  `NativeModule`, native-created channels, list and map values) is the Rust
  API that Rust natives, the interactive server, and the firmware use. It
  stays as is.
- New typed features: structs, buffers, constants, `str` and `bytes` return
  values, and producers on embedded targets.
- Performance work, which stays deferred
  ([VM optimization item 11](vm-optimization.md#11-append-to-uniquely-owned-collections-in-place-deferred)).

## What exists today

- **Loader.** `crates/slug-desktop-loader/src/ffi_prototype.rs` (about 1,800
  lines) loads 0.13 libraries. Its `ffi_prototype/loader.rs` holds the
  `dlopen`/`LoadLibrary` lease that the typed loader also uses
  (`library_lease`, `retain_library_for_foreign_worker`). `clutch.rs` parses
  the `abi = "slug-ffi-prototype/0.13"` manifest form. The crate exports
  `ABI_PROFILE`, `FfiPrototypeError`, and `FfiPrototypeLibrary`.
- **Headers.** `include/slug_ffi_prototype.h` and the helper SDK
  `include/slug_ffi_helpers.h`, the only files in `include/`, documented in
  `docs/reference/ffi-helpers.md`.
- **Tests.** `crates/slug/tests/ffi_prototype.rs` (37 tests), 16 C fixtures
  in `crates/slug-vm/tests/ffi/` that only that file compiles, and the
  prototype clutch copies in `crates/slug/tests/ffi_prototype/`. The file
  also holds benchmarks: the 0.13 callback baseline, the VM scalar-call
  controls, and the typed-versus-prototype end-to-end comparisons for the
  filesystem, SQLite, and standard input.
- **Build and CI.** `make test-ffi-prototype` runs every native test suite,
  not just the prototype one, in the CI job named `ffi-prototype`.
  `make bench-ffi-prototype-scalar` and
  `scripts/benchmark-ffi-prototype-scalar.sh` record local timings.
- **Records.** The typed-calls record lists twelve prototype records whose C
  ABI mechanics are superseded "once step 4 lands", and keeps four producer
  records as the semantic authority for `events`. About a dozen other clutch
  and native records mention the prototype as their mechanism.

## Decisions

1. **What is removed.** Recommendation: the prototype loader and its public
   exports, the `abi` manifest form, `include/` with both headers,
   `docs/reference/ffi-helpers.md`, `crates/slug/tests/ffi_prototype.rs` and
   its clutch copies, and the C fixtures in `crates/slug-vm/tests/ffi/`.
   `slug-vm` does not change, as stated under Out of scope.
2. **Coverage before removal.** Several prototype tests guard loader
   behavior that has no typed desktop test yet. Examples include unloading the
   library at shutdown and reporting `native.plugin_inactive` from a stale
   function afterwards, releasing resources during error unwinding and VM
   teardown, keeping same-named resources of two modules apart, rejecting a
   resource of the wrong type, and serving pure and native modules from one
   clutch. Recommendation: audit every prototype test first. Port each
   behavior that typed clutches need and no typed test covers, as a typed
   desktop test, in its own commit before anything is deleted. Drop the
   tests for prototype-only features (descriptor validation, member-key
   dispatch, list and map builders, destructor transfer), without porting.
3. **A manifest that still uses `abi`.** Recommendation: a checked manifest
   error that names the cause, for example "the 0.13 prototype native ABI
   was removed; declare typed bindings under `[native.functions]`". A
   generic unknown-key error would leave a third-party clutch author
   guessing. The step 3 test that rejects mixing `abi` with typed keys
   becomes a test of this error. This changes a diagnostic and needs your
   acceptance.
4. **Shared dynamic-library code.** Recommendation: move `LoadedLibrary`,
   `library_lease`, and `retain_library_for_foreign_worker` into a
   `dynamic_library` module of `slug-desktop-loader`, and rename
   `FfiPrototypeError` to `NativeLibraryError`. The crate keeps
   `unsafe_code` allowed only in that module and in `typed_native`. Removing
   the three public exports is an API break in a pre-release crate, with no
   caller outside the repository.
5. **Benchmarks.** Recommendation: move the VM scalar-call controls into
   `typed_native.rs` beside the typed scalar benchmark, which compares
   against them. Delete the 0.13 callback baseline and the three
   typed-versus-prototype comparisons, whose numbers stay recorded in the
   completed step plans and the dispatch spike results.
6. **Names.** Recommendation: rename `make test-ffi-prototype` to
   `make test-native`, `make bench-ffi-prototype-scalar` to
   `make bench-native-scalar` (with its script), and the CI job
   `ffi-prototype` to `native`. A rename that removes the last "prototype"
   name from the build is part of the removal, not a separate change.
7. **Decision records.** Recommendation: no new record. The typed-calls
   record already decides the removal, so this step applies its stated
   consequence. Rename the twelve records it lists to `-superseded.md`, mark
   them `Superseded` with a link to the typed record that replaces each, and
   add `Supersedes` links to those typed records. Then audit the other
   records that mention the prototype:
   - supersede the ones whose decision is only prototype mechanics, such as
     the library descriptor and enabling the prototype;
   - keep the ones whose rule still holds, such as clutch-owned native
     support, deterministic shutdown, the library layout, staging, and the
     `slug.core` scope, and add a cross-reference noting that typed bindings
     now implement them.

   I'll bring you the classification before renaming anything. Every link
   to a renamed record is updated.
8. **The 0.13 producer plan.** `docs/planning/ffi-channel-producer-v0.13.md`
   describes finished work. Recommendation: move it to
   `docs/planning/completed/` and update its links.

## Phases

### 1. Coverage audit

- [ ] Classify each test in `ffi_prototype.rs`: covered by a typed test,
  needed and uncovered, or prototype-only.
- [ ] Port each needed, uncovered behavior to a typed desktop test, with the
  fixture C it needs.

### 2. Remove the loader

- [ ] Move the shared dynamic-library code into `dynamic_library` and rename
  its error type.
- [ ] Delete the prototype loader, its exports, and the `abi` manifest form,
  and report the decision 3 error for a manifest that uses `abi`.

### 3. Remove the tests, fixtures, headers, and SDK

- [ ] Move the scalar-call controls to `typed_native.rs`, and delete
  `ffi_prototype.rs`, its clutch copies, and the C fixtures.
- [ ] Delete `include/`, and drop `-I include` from any remaining build
  command.
- [ ] Rename the make targets, the benchmark script, and the CI job.

### 4. Documentation and records

- [ ] Remove the prototype sections from `native-abi.md` and
  `experimental-clutches.md`, delete `ffi-helpers.md`, and update the docs
  index, the README, and `AGENTS.md`.
- [ ] Supersede and cross-reference the decision records as classified, and
  move the 0.13 producer plan to `completed/`.

## Exit gate

- No code, build file, or current documentation refers to the prototype ABI,
  its headers, or its helper SDK. Historical records and completed plans
  keep their text.
- Every loader behavior the audit marked as needed has a typed test.
- `slug-vm` is unchanged and keeps `unsafe_code = "deny"`.
- `make check` passes, and CI passes on macOS, Linux, Windows, and ESP32-S3.
