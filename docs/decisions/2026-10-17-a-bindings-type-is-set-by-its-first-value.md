# A binding's type is set by its first value

## Status

Accepted

## Context

An unannotated binding's type comes from its initializer, and an unannotated
struct field's from its default. Two things made that rule fail in practice:

- A `nil` first value fixed the type to `nil`. `var best = nil` could never
  hold a number, and `struct { error = nil }` could never hold a message,
  although `nil` is how Slug code says "no value yet". The job runner
  project's `Outcome` struct hit this at once.
- A `var` had a single type that assignment overwrote. After `x = 5`, an
  annotated `var x:num|nil` rejected `x = nil`, so an annotation held only
  until the first assignment.

See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

A binding's type is set by its first value: an annotation when present,
otherwise the initializer or default.

- A `var`, and a struct field, which construction and copying can replace,
  has a declared type that every assignment must conform to. A `nil` first
  value declares `any|nil`; for a `var`, see
  [A `nil`-started var takes its first assignment's type](2026-10-18-a-nil-started-var-takes-its-first-assignments-type.md).
- A `val` is assigned once, so a `nil` `val` stays exactly `nil`.
- A `var` also has a current type, which assignments and nil tests refine for
  the code that follows, without changing the declared type. A nested function
  reads a captured `var` by its declared type, since it may run after any later
  assignment.

## Consequences

### Positive

- Optional values work without annotations, in `var`s and struct fields.
- An annotated `var` keeps its annotation.

### Negative

- Reading a `nil`-initialized `var` or field where no assignment or nil test
  is in view gives `any|nil`, which must be narrowed before it goes where a
  concrete type is required.

### Neutral

- A concrete first value still fixes the type: `var count = 0` rejects
  `count = nil`. `var count:num|nil = 0` allows it.
- Measured on 194 Slug files, from the standard library to every library the
  evaluation agents wrote, no file that compiled before fails, and none that
  failed compiles.
- Values the checker cannot infer are still widened to `any|nil` when bound.
  Keeping them dynamic is a separate decision, because binding and field
  annotations are not checked at runtime.

## Migration

None. Every program that compiled before compiles and behaves the same.
