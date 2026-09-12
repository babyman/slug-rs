# Canonical interactive protocol diagnostics

## Context

The interactive-session server needs machine-readable failures for parser,
semantic, runtime, malformed-request, and host failures. `SourceError` and
`RuntimeError` are structured Rust types, but neither is a serialized protocol
contract and protocol failures have no corresponding Slug error value.

## Decision

The interactive protocol uses a versioned canonical diagnostic projection with
`source`, `runtime`, `protocol`, and `host` categories. Source and runtime
projections retain their category-specific kind, message, location, frames,
causal runtime errors, native details, and a display-safe summary of thrown or
native data values. Protocol and host failures use the same envelope.

The wire representation is a projection, not a serialization of Rust error
structs or arbitrary Slug values. It must not collapse failures to display text.

## Consequences

Protocol clients receive one consistent error shape without acquiring a Rust
embedding dependency. The initial value summary is intentionally insufficient
as a general result-value encoding; later submission work must separately
define that contract. Tests must cover source, runtime, protocol, and host
diagnostic serialization.

## Migration

None.
