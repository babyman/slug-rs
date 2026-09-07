# Clutch-Owned Native Support

## Context

The original exploded-clutch manifest attached `native = true` to one module.
That made a clutch appear to be a wrapper for a native extension and prevented
one library from supporting several modules, despite a clutch being the unit
that composes and distributes modules.

## Decision

A clutch is a container of one or more Slug modules plus optional supporting
resources, metadata, and one optional native implementation library. `[native]`
belongs to the clutch, not to a module. Its library initializes once for the
clutch and may register module-qualified foreign functions and resources for
any subset of the clutch's modules.

Module entries contain only their source (or a host-plugin entry in the legacy
embedding path). A module with no `foreign` declarations is pure Slug whether
or not its clutch has a native library. Modules validate their own declarations
against the clutch-owned registration scope when they are loaded.

## Consequences

The loader keys active plugin ownership by clutch root, so imports of multiple
modules reuse one library lease and one shutdown lifecycle. The registrar
validates every registration against the clutch's full module set. Existing
single-module descriptor libraries remain valid and simply support a subset.

This does not add archive clutches, `.cslug`, dependency resolution, multiple
native libraries per clutch, or live unloading. It supersedes the one-native-
module-per-clutch restriction in
[`2026-09-07-exploded-clutch-native-library-layout.md`](2026-09-07-exploded-clutch-native-library-layout.md).

## Migration

Remove `native = true` from every module entry and retain the clutch-level
`[native]` table. Existing libraries and source imports need no other change.
