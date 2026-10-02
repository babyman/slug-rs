# Keep fixture tooling outside the VM

## Context

`slug-vm` retained fixture-sidecar TOML parsing and a process-spawning fixture
runner even after source execution, configuration, and CLI policy moved into
the executable host. Those tools made the runtime library depend on filesystem,
process, timing, and TOML facilities unrelated to bytecode execution.

## Decision

Fixture metadata and `FixtureRunner` live in `slug`, alongside the
`slug-fixtures` binary that invokes them. `slug-vm` contains only runtime and
bytecode concerns. The `slug` package keeps `slug-nil-loader` as a test-only
dependency because its executable implementation does not use that loader.

`DesktopActivation` remains private to `slug-desktop-loader`; resolvers expose
it only through the loader activation-transaction trait.

## Consequences

The VM's normal dependency graph contains no loader, TOML, filesystem, or
process fixture tooling. Fixture validation continues at the executable-host
boundary, where the fixture runner and its binary belong. Desktop activation
representation cannot become an accidental Rust API.

## Migration

No Slug source behavior, fixture format, CLI command, or native activation
order changes. Rust tests importing fixture metadata use the `slug` crate.
