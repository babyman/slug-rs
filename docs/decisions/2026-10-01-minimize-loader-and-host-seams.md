# Minimize loader and host seams

## Context

The crate reorganization established separate VM, frontend, loader, desktop,
and restricted-host packages. A post-migration review found three remaining
cross-layer costs: the VM stored the loader's `ModuleKey`, module graph hosts
implemented desktop activation lifecycle methods even when they had no
activation, and the restricted host duplicated the frontend's generic
in-memory host assembly. The retired `slug-server` wrapper package also
remained in the workspace.

## Decision

`slug-vm` retains a module identity only as opaque text for the VM host import
callback; it does not depend on or re-export loader contracts. `slug-loader`
owns a resolver-provided activation transaction, and `slug-frontend` sequences
that transaction around module initialization without knowing its concrete
representation. `ModuleGraphHost` retains only VM-specific graph services.

The restricted-host crate uses `slug-frontend::ModuleHost` with `slug-nil-loader`
and explicit configuration rather than implementing a second graph host. The
`slug-server` compatibility package and executable are removed; `slug --server`
is the sole supported server entrypoint.

## Consequences

The runtime dependency graph no longer includes `slug-loader`, non-desktop
hosts have no activation no-op implementations, and restricted-host behavior
uses the same generic graph-host path as other in-memory embeddings. Resolver
implementations that expose an activation lease must implement its transaction;
resolvers without activations use the default no-op path.

## Migration

This changes only unstable Rust implementation seams. Slug source semantics,
desktop import precedence, native activation ordering, and `slug --server`
behavior are unchanged. Consumers of the removed `slug-server` package must
invoke the `slug` executable with `--server`.
