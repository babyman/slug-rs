# Crate organization and import loaders baseline

This inventory freezes the observable behavior and Rust seams before the crate
split described in
[Crate organization and import loaders](crate-organization-and-import-loaders.md).
It is a migration aid, not a compatibility promise for the current public
Rust API.

## Baseline verification

On 2026-09-26, all pre-migration focused suites passed with no pre-existing
failures:

| Command | Result |
| --- | --- |
| `cargo test -p slug-vm --features metrics --test vm` | 111 passed |
| `cargo test -p slug-vm --features metrics --test cli` | 247 passed |
| `cargo test -p slug-vm --features metrics --test module_loader` | 57 passed |
| `cargo test -p slug-vm --features metrics --test configuration` | 3 passed |
| `cargo test -p slug-server --test interactive_server` | 36 passed |
| `cargo test -p slug-vm --test ffi_prototype` | 30 passed |

## Observable behavior owners

| Behavior | Regression suite |
| --- | --- |
| Explicit-path, project-root, and installed bare-name entry lookup | `crates/slug-vm/tests/cli/basics.rs`: `executes_a_bare_program_name_from_the_library_directory` |
| Source provider precedence over Clutches | `crates/slug-vm/tests/module_loader.rs`: `existing_source_providers_take_precedence_over_clutches` |
| Relative and library-root import resolution | `crates/slug-vm/tests/module_loader.rs`: `resolves_importer_relative_source_and_library_roots` |
| Module graph identity, cache reuse, live exports, and static import snapshots | `crates/slug-vm/tests/module_loader.rs`: `baseline_graph_preserves_resolution_cache_liveness_and_import_snapshots` |
| Clutch activation validation, rollback, cleanup, and retained registrations | `crates/slug-vm/tests/module_loader.rs`: `clutch_plugins_reject_unavailable_or_unrelated_registrations`, `clutch_plugin_failures_cleanup_and_do_not_leak_foreign_registrations`, and `clutch_plugins_bind_only_their_declared_module_foreign_functions` |
| Dynamic-native Clutch resource and library lifecycle | `crates/slug-vm/tests/ffi_prototype.rs`: `cleans_up_c_resources_during_error_unwinding_and_vm_teardown` and `unloads_libraries_after_destroying_each_library_state` |
| NDJSON server protocol, diagnostics, state, and output ordering | `crates/slug-server/tests/interactive_server.rs`: `server_binary_keeps_ndjson_on_stdout`, `server_binary_returns_structured_slug_diagnostics_on_protocol_stdout`, and the remaining `server_binary_*` tests |

## Public seam inventory

The move must preserve these data paths without introducing a stable embedding
API. Types named here may move to their owning crate and their Rust paths may
change during this pre-release migration.

| Data crossing a seam | Current definition | Destination owner |
| --- | --- | --- |
| Bytecode source metadata: `SourceSpan`, `SourceId`, `SpanId`, and `Program` | `slug-vm::bytecode` | `slug-vm`; the frontend supplies this runtime diagnostic metadata while lowering |
| Checked execution failures: `RuntimeError`, `RuntimeErrorKind`, `CallFrame`, and `NativeErrorDetails` | `slug-vm::vm` | `slug-vm` |
| Checked source failures: `SourceError` and `SourceErrorKind` | `slug-vm::source` | `slug-frontend` |
| Module request and result data: `ModuleKey`, `ModuleRequest`, `ModuleSource`, `ModuleLoadError`, and `ModuleResolver` | `slug-vm::module` | `slug-loader` |
| Graph cache and initialized module data: `ModuleInstance` plus compiled-program, semantic-snapshot, cycle, and instance caches | `slug-vm::module` | `slug-frontend` |
| VM-facing native registration and call values: `NativeModule`, `NativeFunction`, `NativeArity`, `NativeCall`, `NativeStatus`, `NativeOwnedValue`, resource types, and descriptor errors | `slug-vm::native` | `slug-vm` |
| Desktop policy inputs: configuration, source/library roots, entry lookup, and Clutch repository selection | `slug-vm::configuration`, `slug-vm::host`, and `slug-vm::clutch` | `slug` supplies policy; `slug-desktop-loader` receives selected roots and Clutch activation inputs |
| Interactive wire data: `Request`, `Response`, `IncomingMessage`, `Event`, `EventOrigin`, protocol diagnostics, and `PROTOCOL_VERSION` | `slug-server::interactive` | `slug` server mode |

`Value`, `Vm`, builtin registration, and VM scheduler handles remain
`slug-vm`-owned runtime capabilities. They must not acquire a dependency on a
loader, filesystem, configuration, or interactive protocol type.
