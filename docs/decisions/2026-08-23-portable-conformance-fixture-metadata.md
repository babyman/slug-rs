# Portable conformance fixture metadata

## Context

Slug fixture sources need to describe expected process behavior, module roots,
and execution limits without depending on a Rust test harness or a particular
host operating system.

## Decision

Each `.slug` fixture uses an adjacent `<stem>.fixture.toml` sidecar. Version 1
requires a schema number and outcome, and optionally records exact streams,
relative module and library roots, a positive timeout, and an exact diagnostic.
Invalid or incomplete sidecars are rejected.

The complete schema is defined in
[Conformance Fixtures](../reference/conformance-fixtures.md).

## Consequences

Fixtures can be executed by any conforming runner with explicit expectations.
The runner can distinguish unspecified output from an expected empty stream.
Adding metadata fields requires a schema revision or an explicitly compatible
extension.

## Migration

None. Existing ad-hoc tests remain valid; new portable fixtures require a
sidecar.
