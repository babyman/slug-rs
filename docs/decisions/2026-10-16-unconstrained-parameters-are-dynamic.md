# Unconstrained parameters are dynamic inside their body

## Status

Accepted

## Context

An unannotated parameter whose body supplies no type widens to `any|nil` in
its signature. The body also saw it as `any|nil`, so a function that returned
it, from `fn(out) { out }` to the recursive accumulator helpers Slug code is
built from, had result `any|nil`. That result is not assignable to `str`, so
`val s:str = go(2, "")` and a `:str` function returning `go(...)` failed with
`expected str, got any|nil`, even though every such call was valid. The
specification already says compiler uncertainty must not become the deliberate
`any` type. See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Inside its function body, an unannotated parameter with no body-derived type
is dynamic, the checker's unknown type, rather than `any|nil`. Expressions and
results derived from it are dynamic too, and are checked at runtime.

- The parameter's input signature is unchanged: it still accepts `any|nil`,
  and overload identity still treats `fn(value)` like `fn(value:any|nil)`.
- An explicit `any|nil` annotation keeps its static meaning, so
  `fn(out:any|nil) { out }` still returns `any|nil`.
- Parameters with body-derived types, such as `num` from `value / 10`, are
  unaffected.

## Consequences

### Positive

- Recursive helpers and other functions that pass a parameter through can be
  used where a specific type is required, without annotations.

### Negative

- A mismatch through such a function is found at runtime rather than
  statically, for example `val n:num = identity("a")`.

### Neutral

Inferring a generic signature such as `fn<T>(T):T` would keep those
mismatches static; it remains future work.

## Migration

None. Programs that type-checked before still do.
