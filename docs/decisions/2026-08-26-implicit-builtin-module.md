# Implicit builtin module

## Context

The CLI exposed `println` as an ambient native global, while public library
services now live in named `slug.*` modules. Ambient globals make provenance,
shadowing, documentation, and foreign-signature validation less clear.

## Decision

`slug.builtin` is a host-provided module whose registered exports are injected
into every module. The bundled source declaration file documents those exports
and can be imported explicitly. Injection occurs only when the host has
registered at least one `slug.builtin` foreign binding. A local declaration
takes precedence over the implicit binding.

`slug.builtin` remains deliberately small: only primitives that cannot be
written in Slug belong there. Structured error schemas and ordinary helpers are
future explicit library modules.

## Consequences

Hosts register `println` as `slug.builtin.println`, rather than as a global.
The loader initializes `slug.builtin` before modules that receive its exports.
The module must not implicitly appear as an unbound placeholder on hosts that
do not register it.

## Migration

Existing programs may continue to call `println` without an explicit import.
They can instead use `import("slug.builtin")` when provenance matters.
