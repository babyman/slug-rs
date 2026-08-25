# Declared foreign registry

## Context

The static Rust native facade already gave each descriptor a module-qualified
identity, while source `foreign` declarations were parsed only as metadata.
The first public library module needing that boundary is `slug.channel`.
Installing descriptors as ambient VM globals would let an imported module bind
an unrelated host function with the same local name.

## Decision

The host registers a static native descriptor by its `(module name, local
name)` pair. Before a source module evaluates, each of its `foreign`
declarations resolves against that registry and initializes its own local
binding. The descriptor must have the same arity range as the declaration;
variadic declarations require a variadic descriptor with the same minimum.
Unavailable or incompatible declarations produce a checked
module/foreign-resolution error.
Direct VM globals remain available only for documented builtins and explicit
host globals such as `println`.

The version 0 registry intentionally does not define a dynamic loader or C ABI.
Those remain separate future work under the native ABI contract.

## Consequences

Imported library modules can use declared native functions without inheriting
ambient host bindings. The runtime must preserve module-qualified descriptor
identity and initialize foreign bindings before module top-level execution.
Tests cover successful `slug.channel` binding and an unavailable declaration.

## Migration

Existing source `foreign` declarations that previously acted only as metadata
now require a host registration when their module initializes.
