# Strings have a lexicographic byte ordering

## Status

Accepted

## Context

Ordering comparisons accepted only numbers, so sorting or ordering words
required hand-written character comparison. Agents writing Slug repeatedly hit
`expected numbers, got str and str` and rebuilt the comparison themselves. See
the [agent-facing direction](../planning/agent-facing-direction.md). Ordering
also made an unannotated parameter infer `num`, which would reject strings
passed to a general comparison helper.

## Decision

`<`, `<=`, `>`, and `>=` accept two numbers or two strings.

- Strings compare lexicographically by their UTF-8 bytes, which is Rust's
  `str` ordering and also Unicode scalar value order. It is case-sensitive
  and locale-independent.
- A number against a string, or any other operand, is a checked runtime type
  error, and a statically known incompatible pair is a semantic
  `type_mismatch`. In a match guard it remains a false guard.
- For body-derived inference, an ordering comparison constrains an
  unannotated parameter only through a literal on the other side: a number
  literal infers `num` and a string literal infers `str`. Two unannotated
  operands add no constraint.

## Consequences

### Positive

- Sorting and ordering strings needs no helper code.
- `fn(a, b) { a < b }` works for numbers and strings alike.

### Negative

- Two parameters compared only with each other are no longer inferred as
  `num`, so a mixed-type call is caught at runtime rather than statically.
- Byte order is not a natural-language collation: `"Z" < "a"`.

### Neutral

`<=` and `>=` still lower to a negated `>` and `<`, so they need no new
bytecode.

## Migration

None. Programs that type-checked before keep their behavior, except that two
parameters compared only with each other may now accept strings.
