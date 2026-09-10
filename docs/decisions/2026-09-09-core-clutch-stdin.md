# Include standard input in the core Clutch

## Context

The native `slug.io.stdin` module needs the same bundled installation and ABI
profile as the explicit `slug.std` module. Keeping it in a one-module Clutch
duplicated dynamic-library loading and staging while providing no isolation:
both are process-level standard-library facilities.

## Decision

`slug.core.clutch` owns both `slug.std` and `slug.io.stdin`. One native core
library exports descriptors for both module names. The manifest selects the
core Clutch for each name; `slug.io.stdin` remains its source import identity.

This supersedes the scope statement in
`2026-09-09-core-standard-library-clutch.md` that limited the package to
`slug.std`.

## Consequences

Core Clutch loading and staging compile one native library for the two modules.
The stdin reader keeps its module-local lifecycle state even though the shared
library also serves `slug.std`. Consumers must not infer any relationship
between the modules from their common package.

## Migration

Existing programs continue to use `import("slug.io.stdin")` and
`import("slug.std")`. Development checkouts run the same
`make stage-native-clutches` command, which now stages the stdin adapter as
part of `libslug_core`.
