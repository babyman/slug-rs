# Narrow nilable bindings through control flow

## Context

Slug already narrows a binding inside a successful type-constrained match case,
but ordinary nil checks do not carry that fact into an `if` branch. As a
result, a value declared `str|nil` cannot be passed to a `str` parameter even
immediately after `if (value != nil)`.

## Decision

With `-type-check`, a direct lexical binding compared to `nil` using `==` or
`!=` is narrowed in the corresponding branch. For `value != nil`, the true
branch excludes `nil` and the false branch has type `nil`; `value == nil`
reverses those facts. The facts apply to the right operand of short-circuit
`&&` and `||` when that operand is evaluated.

The checker analyzes each branch in a cloned environment. Facts, declarations,
and assignment-side callable changes do not leak from an `if`, logical right
operand, match case, or guard into its enclosing environment. A mutable
binding's inferred type survives an `if` only when both continuing paths agree
on it. A successful match guard contributes its facts to that case result. An
`if` result remains the union of its branch result types; logical expressions
retain the union of their possible operand results.

Only direct name-versus-`nil` comparisons refine today. Literal equality,
ordering comparisons, arbitrary predicates, destructuring, and incompatible
assignment joins remain conservative until their own rules are specified.

## Consequences

- Nilable APIs become practical without runtime coercion or type tags.
- Short-circuit expressions can safely use a checked non-nil value on their
  evaluated right-hand side.
- The initial rule is intentionally narrow and predictable; it does not claim
  exhaustive control-flow inference.

## Migration

None. This adds optional static precision and leaves runtime control flow
unchanged.
