# Constrain whole match patterns with types

## Context

Slug patterns already express nil, list, map, and partial-field matching.
The former `Schema {field}` form made schema identity a special pattern form
instead of using the language's improving type vocabulary. Adding separate
type-only cases would duplicate existing structural patterns and leave the
relationship between matching and type narrowing unclear.

## Decision

A `match` case may attach one postfix type constraint to its complete pattern:

```slug
usr @ {age: 43, name}: struct<User> => name
```

The structural pattern and the constraint must both match before the case
guard runs. Constraint failure is an ordinary failed match. Successful
constraints narrow case-local bindings when optional type checking is enabled.

The constraint is deliberately restricted to whole case patterns. It does not
apply to declarations or nested patterns, preserving declaration annotations
and map-field syntax without ambiguity.

Direct value categories, `struct<Name>`, unions of runtime-checkable types,
and recursive `list<T>` and `map<K, V>` types are runtime-checkable. Function
signatures, task/channel payloads, tuple types, and generic parameters are
not: using them as a case constraint is a source error. `struct<Name>` uses
the exact schema identity denoted by `Name`; its binding must resolve to a
schema.

The special `Schema {field}` syntax is replaced by `{field}: struct<Schema>`.
`_: struct` is the corresponding any-struct case.

## Consequences

- Schema identity, collection element checks, and primitive-kind checks share
  one pattern mechanism.
- Existing patterns remain responsible for shape and binding; types provide a
  check and checker narrowing rather than another set of pattern forms.
- Recursive collection constraints may inspect every collection member.
- Runtime metadata is not introduced merely to support function, task, or
  channel generic constraints.

## Migration

Rewrite struct patterns using a map pattern plus a schema constraint:

```slug
User {name} => name
```

becomes:

```slug
{name}: struct<User> => name
```

The current Rust subset temporarily retains the former spelling until it
implements the new rule. This record supersedes the source-pattern syntax in
[Match structs by schema identity](2026-08-23-struct-pattern-semantics.md).
