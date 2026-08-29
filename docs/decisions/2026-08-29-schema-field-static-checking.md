# Retain schema field metadata for static checking

## Context

`schema` and `struct<S>` establish nominal struct identity, but optional
checking cannot use a known schema's declared fields. Consequently, invalid
known-schema construction, copy replacement, and field access remain runtime
errors even when the checker has enough information to prove them.

## Decision

Keep schema field names, annotations or inferred defaults, and required-field
status in semantic binding metadata. The metadata follows direct schema
bindings, aliases, and module export snapshots; it is not runtime value or
bytecode metadata.

Under `-type-check`, construction through a statically known schema binding
checks its target is a schema, supplied field names, duplicate fields, required
fields, and supplied field values. A known `struct<S>` value checks copy field
names and replacement values and gives direct string field access the declared
field type. Unknown or dynamically selected schemas retain generic `struct`
behavior and the established checked runtime errors.

`struct<S>` annotations resolve `S` lexically to a schema binding; another
value produces a source diagnostic. Bindings carry a stable internal schema
identity separate from the spelling used at an annotation site. Construction,
aliases, imports, copies, and field access retain that identity, so shadowing
cannot redirect field metadata.

## Consequences

- Field annotations become useful at construction, copying, and reads without
  changing struct runtime values.
- Imported schemas provide the same field precision as local schemas.
- Schema aliases are assignment-compatible with the original `struct<S>`;
  a non-schema `struct<S>` argument is now a static diagnostic.
- Dynamic schema code remains valid and conservatively typed.

## Migration

Programs run with `-type-check` may now report known construction or field
mistakes that would previously have failed at runtime. Programs without that
flag are unchanged.
