# A repeated map-literal key keeps its last value

## Status

Accepted

## Context

Maps are unordered values whose keys are unique under
[map-key equivalence](2026-09-15-map-key-equivalence.md), and
[map merge overwrites a matching key](2026-09-15-unordered-map-semantics.md).
Map literals did not follow that rule. `{a: 1, a: 2}` stored both entries:
lookup returned `2`, but `len` reported `2`, `keys` listed `"a"` twice, the
map printed both entries, and it compared unequal to `{a: 2}`. Keys that
are equal but spelled differently, such as `[1]` and `[1.0]`, behaved the
same way. The VM's indexed map representation needs unique keys, so it
forced a decision.

## Decision

A map literal, or a map a native function builds, that repeats a key keeps
the last value for that key, as if its entries were merged in source order.
All entries are still evaluated in source order. The result is an ordinary
map with unique keys.

## Consequences

### Positive

- Every map holds unique keys, so `len`, `keys`, equality, and formatting
  agree with lookup.
- Literal construction matches merge: `{a: 1, a: 2} == {a: 1} + {a: 2}`.

### Negative

- A repeated literal key is still accepted silently rather than diagnosed.
  The compiler can add a warning for repeated constant keys without
  changing this rule.

### Neutral

Lookups already returned the last value, so only programs that repeat a key
and then count, list, print, or compare the map observe a change.
