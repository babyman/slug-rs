# Propagate known calls and nil partitions in body-derived inference

## Context

Body-derived parameter inference currently records direct numeric requirements
and finite operator alternatives from a function's own expressions. That is
too narrow for two ordinary bodies:

```slug
val divide = fn(value) { value / 10 }
val apply = fn(value) { divide(value) }
```

`apply` should expose the same `num -> num` relationship as its direct known
callee. Likewise, a nil guard can make an otherwise numeric operation safe for
a nilable input:

```slug
val divideOrZero = fn(value) {
  if (value == nil) { 0 } else { value / 10 }
}
```

The input relationship is `num|nil -> num`, not `num -> num`. Treating either
case as caller-derived inference would be unsound and would make callers alter
function meaning.

## Decision

Unannotated parameters MAY derive constraints from statically known direct
calls in their declaring body and from direct nil-control-flow partitions.

### Known direct calls

When a direct call resolves to exactly one statically known callable signature,
passing an unannotated parameter to one of its ordinary parameter positions
constrains that argument parameter by the resolved input type. The callee's
signature must already be available; callers never contribute facts to the
callee.

The rule applies only to a proven direct callee and a statically bound argument
position. Dynamic or structural callees, dynamic arguments, spreads, unresolved
overload sets, and calls involved in an unsupported inference cycle do not add
a constraint. They retain ordinary dynamic-call behavior.

### Nil partitions

A direct comparison of an unannotated parameter with `nil` partitions its
requirements. The `value == nil` branch contributes `nil`; its opposite branch
contributes the non-nil requirements established by reachable expressions.
The inverse applies to `value != nil`. Joining the reachable partitions forms
the parameter type.

Thus `if (value == nil) { 0 } else { value / 10 }` infers `num|nil` for
`value`. Only direct comparisons of the parameter itself receive this rule.
Aliases, arbitrary predicates, and facts that cannot be joined conservatively
remain broad.

### Boundaries

This is body-derived inference: it improves static signatures and diagnostics
without adding runtime validation, coercion, function-entry contracts, or
caller-selected specialization. Initial implementation must not solve mutually
recursive inference cycles; it widens those cases and retains ordinary dynamic
call behavior until a separate decision defines convergent cycle handling.

## Consequences

- Simple wrappers retain useful parameter and result signatures without copying
  annotations.
- Nil-guarded numeric functions accept statically known `nil` and numbers but
  reject statically known incompatible non-nil values.
- Direct-call inference needs to use already-published callable metadata before
  ordinary body checking reports a mismatch.
- Precision remains intentionally limited at dynamic, structural, spread, and
  cyclic boundaries.

## Migration

Known calls to newly narrowed wrappers may become semantic errors when their
arguments are provably incompatible. Dynamic calls keep their checked runtime
behavior. No source syntax changes.
