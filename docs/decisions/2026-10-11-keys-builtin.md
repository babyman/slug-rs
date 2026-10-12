# `keys` is a builtin

## Status

Accepted

## Supersedes

- [Package the core standard library as a Clutch](2026-09-09-core-standard-library-clutch-superseded.md)

## Context

`keys(map)` returns a map's keys. Slug code cannot enumerate a map's keys,
and neither can a typed native binding, which carries no maps. Today `keys`
exists only as `slug.std.keys`, a foreign function implemented by the
`slug.core` clutch's 0.13 prototype library. Removing the prototype, step 4 of
[Typed native calls](2026-10-10-typed-native-calls.md), would remove it.
Listing a map's keys is also essential beyond `slug.std`: embedded programs
and any code that walks a map need it.

Slug has two kinds of builtin:

- **VM builtins**, `cfg`, `int`, and `stacktrace`, which the VM installs as
  globals in every program; and
- **host builtins**, `print`, `println`, `len`, `chan`, and `close`, which
  `slug.builtin` declares as `foreign` and each host registers. They are
  implicitly available in every module and importable from `slug.builtin`.

A module-level declaration binds its name throughout the module before its
initializer runs, so a module cannot re-export a VM builtin under the same
name: `export val keys = keys` fails with "binding `keys` is not
initialized".

## Decision

`keys(m:map):list` is a host builtin declared in `slug.builtin` beside `len`,
in the desktop `lib/slug/builtin.slug` and the embedded
`slug.embedded.clutch` contract. Every host that provides `slug.builtin`
registers it: the CLI, the interactive server, and the ESP32-S3 firmware.

`keys` returns the map's keys as a new list, in the map's enumeration order,
which is unspecified, as [Maps](../language/maps.md) already states. Each key
keeps its value type. A non-map argument is a checked `native.type` error.

`slug.std` no longer exports `keys`: with the builtin available everywhere, a
second name for it is redundant. The `slug.core` prototype library no longer
implements it.

## Consequences

`keys` is available in every module without an import, on desktop and
embedded hosts alike, and survives removing the prototype. A host that
provides `slug.builtin` must register `keys`, as it already must register
`len`.

`slug.std.keys` was documented as returning insertion order; the builtin does
not promise an order. The value model keeps entries in insertion order today,
but the language leaves map order unspecified.

## Migration

A program that imports `keys` from `slug.std`, as
`val {keys} = import("slug.std")` or `std.keys(map)`, must drop that import and
call the builtin `keys(map)`; destructuring `keys` from `slug.std` now fails.
A local declaration named `keys` takes precedence over the builtin, as for
every implicit builtin. A custom host that provides `slug.builtin` must
register `keys`.
