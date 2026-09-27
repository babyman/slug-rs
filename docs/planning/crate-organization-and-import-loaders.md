# Crate organization and import loaders plan

This task list implements [Crate organization and import
loaders](../decisions/2026-09-26-crate-organization-and-import-loaders.md). It
supersedes the crate-packaging and embedded-loader follow-up work in
[Host-Owned Module Resolution](host-owned-module-resolution.md); the earlier
plan remains the baseline for resolver and Clutch lifecycle invariants.

## Scope and invariants

- Preserve Slug source syntax, desktop import precedence, module identity,
  live bindings, cyclic initialization, static import snapshots, and checked
  diagnostics.
- Keep the module graph cache in `slug-frontend`, not in any loader.
- Keep VM construction, builtin registration, entry lookup, configuration, and
  environment policy in `slug` or an embedding host, not in a loader.
- Keep external import resolution and import-scoped native activation in the
  loader boundary. Global host capabilities remain explicit VM registrations.
- `slug-nil-loader` must make no filesystem, environment, configuration,
  Clutch, network, or dynamic-native-library request.
- Retain private, in-process bytecode only. Do not use the migration to add a
  stable Rust API, `no_std` promise, portable `.cslug` implementation, or
  embedded target support.

## Remaining migration sequence

Complete the remaining phases through these dependency-ordered, separately
tested commits. Do not check an item off until its stated gate passes.

- [ ] Add narrow VM-owned bytecode-builder and host-callback contracts needed
  by the frontend, with focused VM tests. Keep them private or explicitly
  unstable; they are not a new embedding compatibility API.
- [ ] Move source parsing, semantic analysis, lowering, and checked source
  diagnostics to `slug-frontend`. Keep any compatibility re-exports temporary
  and identify their removal commit.
- [ ] Move the module graph cache and runtime to `slug-frontend`, then prove
  graph identity, cycles, live exports, static snapshots, and checked failures
  with an in-memory resolver.
- [ ] Move desktop filesystem resolution, Clutch discovery, native activation,
  leases, and cleanup to `slug-desktop-loader`.
- [ ] Move CLI/configuration/entry lookup and interactive server assembly to
  `slug`; update `slug-repl` to launch `slug --server`.
- [ ] Add the nil-loader restricted-host harness and a dependency inspection
  proving that path excludes desktop-loader and Clutch code.
- [ ] Move suites and documentation to their final owners, remove transitional
  re-exports, run `make check`, and complete the delivery checklist.

## 0. Freeze the observable baseline

- [x] Run the existing VM, CLI, module-loader, configuration, interactive, and
  native-Clutch tests before moving code; record any pre-existing failures.
- [x] Add focused assertions for the current desktop entry lookup, import
  precedence, module graph cache behavior, Clutch activation lifecycle, and
  `slug-server` protocol behavior.
- [x] Identify every public type crossing the intended crate seams, especially
  source spans, source/runtime errors, module identities, native registrations,
  and interactive protocol data.

**Gate:** complete. The baseline tests pass and every observable desktop
behavior has an owning regression suite; see
[the baseline inventory](crate-organization-and-import-loaders-baseline.md).

## 1. Establish the workspace seams

- [x] Add workspace packages `slug-frontend`, `slug-loader`,
  `slug-nil-loader`, `slug-desktop-loader`, and `slug` without changing the
  executable behavior.
- [x] Declare one-way dependencies: `slug-frontend` depends on `slug-vm` and
  `slug-loader`; both loader implementations depend on `slug-loader`; `slug`
  composes all selected libraries. Avoid dependencies from `slug-vm` to any
  higher layer and from `slug-loader` to desktop policy.
- [x] Give each new crate a crate-local ownership note and focused test target.
- [x] Keep transitional re-exports short-lived and mark their removal point in
  the migration commits rather than creating a second permanent public API.

**Gate:** complete. `cargo build --workspace` succeeds with the new empty
seams and no dependency cycle. No transitional re-exports are required until
the next migration steps move concrete types.

## 2. Make `slug-vm` a runtime-only library

- [x] Move dynamic values, bytecode, VM execution, checked runtime errors,
  scheduler support, and VM-facing native registration mechanisms into
  `slug-vm`.
- [ ] Move lexing, parsing, ASTs, semantic analysis, source lowering, source
  compilation, source diagnostics, configuration, module loading, host setup,
  and CLI code out of `slug-vm`.
- [ ] Replace VM references to source/host types with the smallest shared
  metadata or callback contract required for checked diagnostics and imports.
- [ ] Move private-bytecode/runtime tests with the VM and prove the VM can be
  built without the frontend or either concrete loader.

**Gate:** `slug-vm` has no source parser, filesystem, environment, Clutch, or
command-line dependency, and its focused VM suite passes.

## 3. Move the language frontend and module graph

- [ ] Move syntax, AST, semantic analysis, lowering, source errors, and source
  compilation into `slug-frontend`.
- [ ] Move compiled-program, semantic-snapshot, resolving-cycle, and module
  instance caches into a frontend-owned module graph runtime.
- [ ] Make both static import inspection and runtime `import()` consult the
  same injected `slug-loader` contract.
- [ ] Preserve virtual `slug.builtin` behavior as an explicit host/VM facility,
  not a filesystem special case and not a nil-loader exception.
- [ ] Move source, module graph, and source-diagnostic tests to the frontend;
  keep observable runner assertions at the executable boundary.

**Gate:** a non-filesystem test loader proves relative graph behavior, cache
identity, cycles, live exports, static snapshots, and checked import errors.

## 4. Extract import loaders

- [x] Move `ModuleKey`, `ModuleRequest`, `ModuleSource`, `ModuleLoadError`,
  resolver traits, and import-scoped activation contracts to `slug-loader`.
- [x] Implement `slug-nil-loader` as a deny-all external resolver with focused
  tests proving every explicit external import fails through a checked module
  error and no desktop mechanism is linked or invoked.
- [ ] Move importer-relative, project-root, library-root, Clutch discovery, and
  native activation behavior to `slug-desktop-loader`.
- [ ] Pass already-selected roots and desktop policy into `slug-desktop-loader`;
  it must not read `SLUG_HOME`, fixture overrides, process arguments, or
  configuration files on its own.
- [ ] Preserve transactional native registration, activation cleanup, library
  leases, and shutdown behavior in desktop-loader tests.

**Gate:** the same frontend graph suite passes with both a test resolver and
the nil loader; desktop loader tests retain current resolution and native
lifecycle behavior.

## 5. Create the `slug` executable and server mode

- [ ] Move the existing `slug` runner from `slug-vm` into the `slug` package.
- [ ] Move `slug-server` interactive server implementation into `slug`; expose
  its existing protocol through `slug --server`.
- [ ] Make `slug` own CLI parsing, environment discovery, immutable
  configuration assembly, entry-program lookup, desktop-loader construction,
  builtin registration, and VM/frontend assembly.
- [ ] Update `slug-repl` to launch `slug --server`; retain an explicit override
  for test and distribution launchers where needed.
- [ ] Preserve CLI exit status, human and JSON diagnostics, source locations,
  default and slim runtime behavior, and REPL/server protocol behavior.

**Gate:** the public CLI, server, and REPL suites pass against `slug`; no
binary in `slug-vm` or `slug-server` remains required by a normal installation.

## 6. Prove restricted-host assembly

- [ ] Add a small in-memory entry-source harness using `slug-nil-loader` and
  explicitly selected host builtins.
- [ ] Prove it evaluates source without desktop imports and rejects an explicit
  import as a checked module error.
- [ ] Confirm configuration is supplied by the host harness and no ambient
  environment or filesystem discovery occurs.
- [ ] Add a dependency/build inspection appropriate to the target toolchain to
  prove the nil-loader path does not pull in desktop loader or Clutch code.

**Gate:** the restricted harness has a reproducible no-external-import proof;
it is not yet a `no_std` or ESP32 compatibility claim.

## 7. Complete documentation and delivery

- [ ] Update README package names, commands, and capability wording only when
  each executable/package migration is implemented.
- [ ] Keep the module and host-service language requirements aligned with the
  selected loader model; preserve fixture-host requirements separately from
  restricted-host behavior.
- [ ] Update contributor guidance, test routing, release/install instructions,
  and `slug-repl` distribution notes.
- [ ] Append user-visible changes to `changelog.md`, run `make docs-check` and
  `git diff --check` for documentation slices, then run `make check` before
  the implementation handoff.

## Explicit non-goals

- An ESP32 implementation, `no_std` support, or a promise that all loaders
  work on every embedded target.
- Portable `.cslug` modules or serialization of private VM bytecode.
- Package installation, remote fetching, dependency solving, or Clutch archive
  formats.
- A stable public Rust embedding API.
