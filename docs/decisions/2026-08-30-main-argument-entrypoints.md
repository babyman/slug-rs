# Pass program arguments through main

## Context

`argv()` and `argm()` expose program invocation data through global builtins,
while the runtime separately discovers and invokes `main()`. This splits one
program boundary across unrelated mechanisms. It also leaves no signature-level
indication that a program consumes its invocation arguments.

The earlier [main entrypoint convention](2026-08-23-main-entrypoint-convention.md)
selected a source-ordered callable that could be called without arguments. That
rule cannot distinguish a raw argument list from a parsed argument map, and
ordinary runtime overload dispatch does not use parameter annotations to make
that distinction.

## Decision

This decision supersedes
[Discover main by naming convention](2026-08-23-main-entrypoint-convention.md).

After successful top-level evaluation, a program has exactly one eligible local
entrypoint when it declares one of these signatures:

```slug
val main = fn() { ... }
val main = fn(args:list) { ... }
val main = fn(args:map) { ... }
```

The zero-argument form receives no values. The `list` form receives the raw
arguments after the entry program. The `map` form receives `{ options, positional
}`, using the same option parsing and configuration-key normalization as the
command line.

The one parameter must be required, non-variadic, and annotated exactly `list`
or `map`; its name is not significant. More than one eligible local `main` is a
semantic error. The runtime selects the validated entrypoint signature directly,
rather than using ordinary overload dispatch. `cfg` remains the configuration
access API; `argv` and `argm` are removed.

## Consequences

- Invocation inputs are visible at the program boundary and can use ordinary
  local validation.
- The three entrypoint shapes remain simple without adding generic collection
  annotations to the host-facing contract.
- Duplicate eligible entrypoints fail deterministically instead of depending on
  source order or runtime overload behavior.
- Implementations must retain the selected entrypoint signature and construct
  the corresponding argument value before invocation.

## Migration

Replace `argv()` with `main(args:list)` and `argm()` with `main(args:map)`.
Programs that do not consume arguments retain `main()`. A program that has more
than one of these eligible forms must choose one; helper functions may retain
the name `main` only when they are not eligible entrypoints.
