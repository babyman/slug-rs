# Adopt nominal resource handles and enums

## Context

Native resources already have module and resource-kind identities at runtime,
but the source language exposes only the broad `resource` type. Consequently,
a file can satisfy a future socket API until native code rejects it. The broad
type also provides no clear source contract for a native symbolic option.

Aliases, resource handles, and enumerations need one coherent distinction:
an alias names an existing type, while a resource or enum creates a new type.
Unqualified enum cases would add ordinary names to every consumer scope and
make case origin less clear.

## Decision

Slug adopts top-level `resource`, `enum`, and `type` declarations. Resources
and enums are nominal; aliases are transparent. A resource declaration is an
opaque, module-owned native handle type and replaces the broad source
`resource` annotation. Existing APIs migrate to exact resource names such as
`File`; a union explicitly names every accepted handle kind.

Enums initially have only named, fieldless cases. Cases are values addressed
only through their enum name, such as `SeekFrom.Start`; bare `Start` is never
introduced into source scope. Enum values are neither strings nor numbers.

Type declarations use a compile-time namespace. Exported names retain their
declaring module identity across imports and are selected through an imported
module binding, as in `fs.File`. Enum declarations also provide the qualified
value namespace needed for cases such as `fs.SeekFrom.Start`.

Foreign resource arguments and results are validated against their declared,
module-owned native resource registration at the native boundary.

## Consequences

Foreign signatures become precise and type checking can reject an obvious
wrong-handle call before native execution. Runtime checks still enforce handle
open state, ownership, and dynamic call paths. Closed enums support exhaustive
match checking without stringly typed constants.

The compiler, module metadata, native registration, bytecode match support,
and native-call API need a shared nominal identity representation. The initial
feature intentionally excludes broad resource supertypes, unqualified enum
cases, numeric enum conversions, flags, enum payloads, generic aliases, and
strong typedefs.

## Migration

This supersedes [Add a broad source `resource` type](2026-09-03-broad-resource-type.md)
and supersedes the nominal-type deferral in
[Keep resource lifecycle explicit and defer nominal resource syntax](2026-09-03-resource-lifecycle-and-typing.md).
It preserves the latter record's explicit-close and fallback-destruction rules.

`slug.io.fs` migrates from `resource` to an exported `File` resource type, and
every existing library or fixture signature using `resource` must migrate to a
declared nominal type. Source compatibility for broad `resource` is not
preserved.
