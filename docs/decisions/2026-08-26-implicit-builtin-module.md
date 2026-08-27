# Implicit builtin module

## Context

The CLI exposed `println` as an ambient native global, while public library
services now live in named `slug.*` modules. Ambient globals make provenance,
shadowing, documentation, and foreign-signature validation less clear.

## Decision

`slug.builtin` is a host-provided module whose registered exports are injected
into every module. The bundled source declaration file is optional: host
registration never depends on it. When present, it documents host exports and
may define additional foundational Slug values; it can always be imported
explicitly when either source or host exports exist. A local declaration takes
precedence over the implicit binding.

`slug.builtin` remains deliberately small: it holds host primitives that cannot
be written in Slug plus universally shared Slug-level foundations such as the
standard `Error` schema. Ordinary helpers remain in explicit library modules.

## Consequences

Hosts register `println` as `slug.builtin.println` with the dedicated builtin
registration API, rather than as a global.
The loader injects registered values before evaluating the optional source
module, so source declarations validate against the same descriptor. The
module must not implicitly appear as an unbound placeholder on hosts that do
not register it and provide no source module.

## Migration

Existing programs may continue to call `println` without an explicit import.
They can instead use `import("slug.builtin")` when provenance matters.
