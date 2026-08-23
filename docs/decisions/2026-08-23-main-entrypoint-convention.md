# Discover main by naming convention

## Context

The `@main` tag makes ordinary metadata control program execution and requires
special validation for duplicate entrypoint tags. Slug already has source-order
callable overloads and default argument binding, which can identify a callable
entrypoint without separate metadata.

## Decision

After successful top-level evaluation, select the first locally declared,
top-level function named `main` that can be invoked without supplied arguments.
A function qualifies when it has no parameters or every parameter has a
default. A required or variadic parameter without a default does not qualify.
Selection uses source declaration order, including overload declaration order,
and invokes only the first eligible function.

Imported functions named `main` do not participate. If no local `main` is
eligible, evaluation ends after the module's top-level statements. Default
expressions on the selected function use ordinary call-binding semantics.

## Consequences

- A program entrypoint is visible from its conventional name and callable
  signature without special tag metadata.
- Multiple eligible `main` overloads are permitted, but source order is
  observable because the first is selected.
- A local `main` with required parameters may coexist with a later eligible
  overload without blocking entrypoint discovery.
- Runtimes and tools must retain source order while discovering callable
  overloads.

## Migration

Remove `@main` and name the intended entrypoint `main`. Give every entrypoint
parameter a default, or declare no parameters. When several local `main`
functions qualify, place the intended entrypoint first.
