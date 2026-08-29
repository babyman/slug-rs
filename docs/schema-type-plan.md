# Schema Type Implementation Plan

This document plans a first-class `schema` type for Slug. It is an
implementation plan, not the normative language rule; the implementation must
first establish that rule in the language specifications and a decision record.

## Intended source surface

```slug
val S: schema = struct {
  name: str
}

val s: struct<S> = S {name: "evan"}

val classify = fn(value) match {
  _: schema => "schema"
  _: struct => "struct"
  _ => "other"
}
```

`schema` denotes a schema value such as `S`. `struct<S>` denotes an instance
created by the exact current schema identity bound to `S`. `schema` has no type
arguments: schema-value identity is already expressible with `^S`, while
instance identity belongs to `struct<S>`.

## Scope and invariants

- [x] Define `schema` as a non-parameterized built-in type annotation and a
  runtime-checkable match constraint.
- [x] Keep `schema` distinct from `struct`: a schema value does not satisfy
  `struct`, and an instance does not satisfy `schema`.
- [x] Infer `schema` from `struct { ... }` expressions.
- [x] Infer `struct<S>` when an initialization target is a statically known
  schema binding named `S`.
- [x] Preserve `struct` as the less precise type for dynamically selected,
  unknown, or non-binding schema expressions.
- [x] Do not add `schema<S>`, user-defined schema generics, runtime coercion,
  or exhaustiveness checking.

## 1. Establish the language rule

- [x] Add an ADR covering the distinction between schema values and struct
  instances, and why `schema` is intentionally non-parameterized.
- [x] Update the language specification, Match and Destructuring supplement,
  Struct supplement, runtime requirements, and EBNF as applicable.
- [x] Define diagnostics for `schema<T>`.
- [x] Update the support manifest as `specified only` until tests pass.

## 2. Type-system representation

- [x] Add `Type::Schema` in `src/source/semantic.rs`, with display,
  assignability, union normalization, and reifiability behavior.
- [x] Resolve the bare annotation name `schema`; reject `schema<...>` with a
  checked source error.
- [x] Make `Value::StructSchema` infer `Type::Schema` and retain
  `Value::Struct` as `Type::Struct(None)`.
- [x] Extend the match-constraint representation with `MatchType::Schema` and
  make it test only `Value::StructSchema`.

## 3. Nominal construction inference

- [x] Record enough semantic metadata for a binding known to hold a schema,
  including its lexical name, without exposing runtime bytecode details.
- [x] In `check_expression` for `StructInit`, inspect the schema expression.
  A direct, statically known binding `S` with type `schema` produces
  `Type::Struct(Some("S"))`; every other schema expression produces generic
  `Type::Struct(None)`.
- [x] Retain the existing runtime schema validation for dynamic construction.
  Static precision must not introduce runtime validation or make dynamic code
  reject earlier than it does today.
- [x] Ensure bindings, imports, aliases, and lexical shadowing preserve or
  deliberately drop nominal schema precision consistently.

## 4. Checker and matcher behavior

- [x] Check `val S: schema = struct { ... }` under `-type-check`.
- [x] Check `val s: struct<S> = S { ... }` under `-type-check`, including
  rejection of values made with a distinct lookalike schema.
- [x] Narrow `_: schema` cases to `schema` and preserve existing `_: struct`
  narrowing for instances.
- [x] Prove that `struct<S>` constraints still compare runtime schema identity
  and that `_: schema` does not match a struct instance.

## 5. Tests, documentation, and handoff

- [x] Add CLI tests for schema annotations, schema/instance classification,
  nominal construction inference, aliases and shadowing, imports, and invalid
  parameterized `schema` annotations.
- [x] Add VM tests for `MatchType::Schema` and checked malformed-bytecode
  behavior where applicable.
- [x] Add type-checker tests for assignability and `struct<S>` inference.
- [x] Mark the support matrix implemented only after source and VM coverage
  pass; update the README and changelog in the same change.
- [x] Run focused tests while iterating, then `make check` before handoff.

## Completion criteria

The feature is complete when `schema` accurately distinguishes schema values
from struct instances, construction through a known schema binding infers the
corresponding `struct<S>` type, dynamic construction remains conservatively
typed, documentation and support status agree, and `make check` passes.
