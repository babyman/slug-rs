# Host-Owned Module Resolution Plan

This task list implements
[Host-Owned Module Resolution](../decisions/2026-09-23-host-owned-module-resolution.md).
It is an internal architecture migration: it must preserve current source
syntax, resolution precedence, initialization semantics, and observable
desktop diagnostics. It does not implement the portable `.cslug` format.

## Scope and invariants

- `import(name, ...)` retains its current source-level behavior and checked
  failure categories.
- An imported module has one opaque, stable identity for compilation caching,
  semantic snapshots, cycles, and initialized instances. That identity is not
  a filesystem-path API in VM code.
- Static-import analysis and runtime `import()` use the same host resolver.
- The desktop host retains its current order: importer-relative, project root,
  library root, then indexed Clutch provider.
- Desktop entry lookup retains its current explicit-path, configured-root, and
  bare-name library fallback rules.
- No VM or filesystem-free module-runtime code reads files, derives importers
  from `SourceSpan.path`, reads environment variables, or parses Clutch
  manifests.
- Native registrations remain module-qualified, transactional, and subject to
  the current shutdown and resource-lifecycle rules.
- The first ROM representation is uncompressed and directly addressable.
  Embedded source is acceptable until the `.cslug` implementation gate is met.

## 0. Establish a compatibility baseline

- [x] Inventory every `ModuleLoader` call site, including CLI entry loading,
  interactive compilation, configuration, native registration, warnings,
  shutdown, Clutch staging, static snapshots, and runtime import.
- [x] Record the existing desktop resolution candidates, success behavior, and
  error text in focused module-loader and CLI tests before moving code.
- [x] Add a small shared module graph covering relative imports, library
  fallback, repeated imports, failed initialization and retry, cycles, exports,
  live bindings, and a static imported callable snapshot.
- [x] Identify the existing native-Clutch lifecycle tests that must continue to
  run only in the desktop configuration.

**Gate:** current desktop module-loader, CLI, configuration, interactive, and
native-Clutch tests pass without behavioral changes.

### Baseline inventory

| Concern | Current owner | Baseline evidence |
|---|---|---|
| Entry lookup | `src/main.rs::read_entry_source` | CLI module tests preserve explicit-path and library fallback behavior. |
| Desktop import candidates | `src/module.rs::ModuleLoader::load` | `resolves_importer_relative_source_and_library_roots`, `source_imports_use_the_configured_library_fallback`, and `source_imports_check_module_name_values_and_loader_failures`. |
| Shared source graph | `ModuleLoader` compiler, snapshot, and instance caches | `baseline_graph_preserves_resolution_cache_liveness_and_import_snapshots`; `cyclic_imports_resolve_predeclared_function_bindings`. |
| Retry and native lifecycle | Clutch staging and loader shutdown | `clutch_plugin_failures_cleanup_and_do_not_leak_foreign_registrations`, the Clutch tests in `module_loader.rs`, and `ffi_prototype.rs`. |
| Configuration and interactive compilation | `ModuleLoader` and `Vm` | `configuration.rs::exposes_cfg_to_program_and_imported_modules` and server interactive-session tests. |
| VM integration | `Vm::import_at` and module-binding construction | `module_loader.rs` import tests and `vm/calls_and_native.rs` shared-loader coverage. |

The baseline graph covers importer-relative source lookup, library fallback,
repeated imports, live exports, and static callable snapshots. The existing
cyclic-import and Clutch retry tests cover the remaining graph requirements;
they remain independent because retry currently requires a staged native
registration failure.

## 1. Define the filesystem-free resolver contract

- [x] Introduce a private or deliberately narrow `ModuleResolver` interface
  accepting a logical requested name and an optional opaque importer identity.
- [x] Define resolved-module data with an opaque cache key, diagnostic label,
  source payload, and optional module activation lease.
- [x] Replace `PathBuf` in core cache, cycle, and instance keys with the opaque
  identity. Keep paths confined to the desktop implementation and diagnostics.
- [x] Redesign resolution errors so a resolver can report an invalid name,
  absence, unreadable payload, or host-specific provider failure without
  requiring a filesystem search-path type.
- [x] Pass the owning program/module identity to runtime `import()`; do not
  infer it from a span path.
- [x] Preserve source paths as diagnostic metadata, not resolution inputs.

**Gate:** a test resolver with no filesystem access can resolve a source module
and produce the same source and module errors expected by the shared graph.

## 2. Extract the filesystem-free module runtime

- [ ] Move compilation caching, semantic snapshots, resolving-snapshot cycle
  protection, initialized instances, and module initialization behind a module
  runtime that depends only on the resolver contract.
- [x] Route `compile_with_resolver` and interactive compilation through that
  runtime so static imports and runtime imports request the same identity.
- [ ] Keep isolated module VMs, predeclared bindings, live exports, metadata,
  warnings, and retry-after-failure behavior unchanged.
- [ ] Separate immutable configuration, native-function registry, resource
  registry, and warning/shutdown sinks from filesystem resolution. Inject each
  as the smallest existing runtime service needed.
- [ ] Preserve the virtual `slug.builtin` module and its current registration
  behavior without making it a filesystem special case.

**Gate:** the existing module-loader suite runs against the extracted runtime
with a test resolver, including static type snapshots and module cycles.

## 3. Reunify desktop entry and import loading

- [x] Implement `DesktopLoader` as the desktop resolver and entry-program
  loader, owning source root, library root, configuration inputs, and desktop
  diagnostic labels.
- [x] Move `main.rs` entry lookup into `DesktopLoader`; make the CLI ask it for
  the entry module rather than calling `fs::read_to_string` directly.
- [x] Preserve explicit entry paths, `SLUG_FIXTURE_MODULE_ROOT`, `SLUG_HOME`
  library fallback, source locations, JSON diagnostics, and exit behavior.
- [x] Implement importer-relative, project-root, and library-root resolution
  using `DesktopLoader`, then retain indexed Clutch lookup as the final
  provider.
- [ ] Keep canonical filesystem identity private to the desktop implementation
  so aliases still share one cached module when current behavior requires it.

**Gate:** all public CLI and module-loader tests pass unchanged; desktop Slug
programs behave identically through the new abstraction.

## 4. Adapt Clutches and native lifecycle

- [ ] Make a resolved desktop Clutch module carry an optional activation lease
  rather than exposing Clutch paths or manifests to the module runtime.
- [ ] Preserve the existing activation sequence: validate/compile source,
  stage registrations transactionally, initialize, remove registrations on
  failure, and retain successful Clutch ownership until shutdown.
- [ ] Keep one native-library lease per Clutch and preserve module-qualified
  foreign/resource validation for every provided module.
- [ ] Confirm an ordinary source or ROM module cannot request a dynamic native
  library merely by naming an import.

**Gate:** `ffi_prototype`, module-loader Clutch, and shutdown lifecycle tests
remain green in the desktop-host configuration.

## 5. Add the embedded resolver

- [ ] Define an embedded module-table format mapping logical module names to
  stable keys, diagnostic labels, and `&'static` source data.
- [ ] Implement a resolver backed solely by that table; missing imports report
  a checked module error and never attempt filesystem or Clutch lookup.
- [ ] Add a fixture adapter that materializes the shared graph as both a
  temporary desktop tree and an embedded table, then runs the same assertions
  against each implementation.
- [ ] Register firmware native capabilities explicitly at host construction;
  prove that pure embedded imports need no storage or dynamic plugin support.
- [ ] Add a target/build check that excludes desktop filesystem and dynamic
  Clutch code from the embedded configuration.
- [ ] Run an ESP32 smoke program importing at least two embedded modules when
  the target harness is available.

**Gate:** `import()` resolves entirely from compiled-in data on the embedded
target, with no filesystem requirement.

## 6. Add `.cslug` only after its independent contract is ready

- [ ] Complete the version-1 binary schema, validator, limits, and fixtures in
  `docs/reference/compiled-artifacts.md` before accepting artifact payloads.
- [ ] Add a `ModulePayload::Cslug(&'static [u8])` path that validates input
  before producing private executable representation.
- [ ] Keep payload bytes uncompressed and directly addressable in the first
  firmware implementation; measure flash size, boot time, and RAM before
  proposing compression.
- [ ] Prove source and artifact modules preserve logical identity, dependency
  checks, diagnostics, and module initialization rules.

**Gate:** malformed or incompatible embedded artifacts fail through checked
module diagnostics, and no private `Program` or opcode layout is serialized.

## 7. Complete documentation and delivery

- [ ] Update the implementation architecture map and crate-local contributor
  guidance to name the resolver, module runtime, desktop loader, and embedded
  loader ownership boundaries.
- [ ] Keep `runtime-requirements.md`, the experimental-Clutch reference, the
  compiled-artifact reference, README capability statement, and changelog
  aligned with the implemented—not merely planned—surface.
- [ ] Run focused module-loader, CLI, configuration, interactive, VM/native,
  and embedded tests while iterating, then run `make check` before handoff.
- [ ] Move this plan to `docs/planning/completed/` only after every exit gate
  is met and the ESP32 import proof is reproducible.

## Explicit non-goals

- A portable `.cslug` format or serialization of private VM bytecode.
- Clutch archives, package installation, remote fetching, or dependency
  solving.
- Dynamic native-library loading on firmware.
- A `no_std` commitment, a new Cargo crate, or a public stable Rust embedding
  API before target evidence requires one.
