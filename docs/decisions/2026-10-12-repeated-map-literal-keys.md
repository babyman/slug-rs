# A map literal must not repeat a key

## Status

Accepted

## Context

Maps are unordered values whose keys are unique under
[map-key equivalence](2026-09-15-map-key-equivalence.md), and
[map merge overwrites a matching key](2026-09-15-unordered-map-semantics.md).
Map literals did not keep keys unique. `{a: 1, a: 2}` stored both entries:
lookup returned `2`, but `len` reported `2`, `keys` listed `"a"` twice, the
map printed both entries, and it compared unequal to `{a: 2}`. Keys that
are equal but spelled differently, such as `[1]` and `[1.0]`, behaved the
same way. The VM's indexed map representation needs unique keys, so it
forced a decision.

A literal lists a map's contents, so a repeated key is almost always a
mistake, such as a copy-paste error or two computed keys that collide. Silently
keeping one value would hide it.

## Decision

A map literal must not repeat a key under map-key equivalence.

- A repeated constant key, such as `{a: 1, a: 2}` or `{[1]: x, [1.0]: y}`,
  is a semantic error: ``map literal repeats key `a` ``.
- A repeated computed key, such as `{[k]: 1, a: 2}` where `k` is `"a"`, is a
  checked runtime type error when the literal is evaluated:
  `map literal repeats key "a"`.

Map merge is unchanged: `{a: 1} + {a: 2}` is `{a: 2}`. A host or native
function that builds a map from entries that repeat a key keeps the last
value, as a merge in that order would, because those host APIs cannot fail.

## Consequences

### Positive

- Every map holds unique keys, so `len`, `keys`, equality, and formatting
  agree with lookup.
- Colliding keys in a literal are reported, not silently dropped.

### Negative

- A program that relied on a later literal entry replacing an earlier one
  must use merge instead.

### Neutral

Constant keys are compared under the same equivalence as runtime keys, so
the compile-time and runtime checks agree.
