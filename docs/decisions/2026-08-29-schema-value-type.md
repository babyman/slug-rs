# Distinguish schema values from struct instances

## Context

Slug exposes schema values and struct instances as distinct runtime values, but
its type vocabulary only describes instances as `struct` or `struct<Name>`.
Consequently, a match can classify a struct instance but cannot classify the
schema value that constructs it, and optional checking cannot state that a
binding holds a schema.

## Decision

Add `schema` as a non-parameterized built-in type. It describes only schema
values produced by `struct { ... }`; it does not describe instances. `_: schema`
matches schema values, while `_: struct` continues to match instances.

Construction through a direct, statically known schema binding `S` infers
`struct<S>`. Thus `val S: schema = struct { ... }` and `val value: struct<S> =
S { ... }` are checked consistently with runtime schema identity.

`schema<T>` is invalid. Schema-value identity remains ordinary value identity,
so `^S` compares a schema value directly and `struct<S>` describes instances
of that schema.

## Consequences

- Static and runtime classification distinguish constructors from constructed
  values without adding a new runtime value representation.
- Known direct schema bindings preserve nominal instance precision; dynamic
  schema expressions remain the less precise `struct` type.
- Match constraints gain `schema` without changing the meaning of `struct`.

## Migration

None. `schema` was previously an unknown annotation name, so valid existing
programs do not change meaning.
