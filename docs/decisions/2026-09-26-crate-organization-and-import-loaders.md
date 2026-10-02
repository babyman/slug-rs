# Crate organization and import loaders

## Context

`slug-vm` currently contains the VM, source frontend, module graph runtime,
desktop import and Clutch resolution, configuration setup, and the `slug`
command-line executable. `slug-server` adds the interactive server, while
`slug-repl` starts that server as a sibling executable. This arrangement makes
the VM depend on source and desktop-host concerns, and makes it difficult to
build a deliberately restricted or embedded host without carrying filesystem,
environment, and dynamic-native-loading code.

[Host-Owned Module Resolution](2026-09-23-host-owned-module-resolution.md)
already separated logical module identity from filesystem paths and extracted
the module graph runtime behind a resolver interface. It deferred a Cargo-crate
split until a target demonstrated the need. The new executable and loader
requirements make that split actionable.

## Decision

The workspace will adopt these ownership boundaries:

- `slug-vm` is a pure runtime library. It owns dynamic values, private
  bytecode, execution, checked runtime failures, and the VM-facing native
  registration mechanisms. It does not parse source, resolve imports, read
  host storage, inspect environment variables, or define command-line policy.
- `slug-frontend` owns source syntax, semantic analysis, lowering, source
  diagnostics, and the module graph runtime. The graph runtime owns compiled
  module, semantic-snapshot, cycle-detection, and initialized-instance caches.
- `slug-loader` defines the intentionally narrow import boundary: logical
  requests, opaque resolved identities, source payloads, checked load errors,
  and optional import-scoped native activation. It does not construct VMs,
  configure processes, or define builtins.
- `slug-nil-loader` implements that boundary by declining every externally
  resolved import. It reads no files, environment variables, configuration, or
  native libraries. A host-provided virtual `slug.builtin` module remains a VM
  host capability rather than an externally resolved import.
- `slug-desktop-loader` resolves desktop imports, including the established
  importer-relative, project-root, library-root, and Clutch provider order. It
  owns import-scoped Clutch and native-library activation, but not process
  environment discovery, configuration construction, entry-program lookup, or
  VM bootstrap.
- `slug` is the runnable language executable. It owns command-line parsing,
  environment and configuration setup, entry-program acquisition, builtin
  registration, VM assembly, and interactive server mode. The former
  `slug-server` executable behavior becomes `slug --server`.

The frontend uses the loader for both static-import analysis and runtime
`import()`. The loader supplies modules and import-scoped activation only; it
does not own language-semantic caches or general host policy.

An embedding may provide source directly and use `slug-nil-loader`. Therefore
an explicit import that the selected loader does not provide fails as a checked
module error. This does not change desktop resolution order, module caching,
live bindings, cyclic initialization, or the source syntax of `import()`.

## Consequences

### Positive

The core VM has a small, testable dependency surface. The executable's host
policy is auditable, and a restricted executable can deny external imports
without incidental filesystem, environment, Clutch, or dynamic-library access.
An embedded host can later use the same frontend and VM with a ROM-backed
loader, while registering only the capabilities it intends to expose.

The module graph's semantic and instance caches have one language-owned home:
`slug-frontend`. Desktop behavior remains independently testable through the
desktop loader, and the nil loader proves that imports are capability-gated
rather than a filesystem assumption.

### Negative

This migration moves public and private Rust types across crates, updates test
ownership, and temporarily increases workspace coordination. The executable
must explicitly assemble configuration, builtins, frontend, VM, and loader
instead of relying on a convenience desktop-host constructor.

The `slug-server` package and executable name are retired in favor of
`slug --server`. Downstream launchers, including `slug-repl`, must be updated
as part of the migration.

### Neutral

This decision supersedes the crate-deferral portion of
[Host-Owned Module Resolution](2026-09-23-host-owned-module-resolution.md).
It does not define a stable Rust embedding API, require `no_std`, implement an
ESP32 target, or make private bytecode portable. The `.cslug` contract remains
the only route to portable compiled-module artifacts.

## Migration

Slug source programs retain the same syntax and desktop import behavior. An
explicit import in a nil-loader host reports a checked module error. Rust
embedding APIs remain pre-release and may change as types move to their owning
crates. The executable migration must retain the documented desktop CLI
behavior and provide `slug --server` before removing `slug-server`.
