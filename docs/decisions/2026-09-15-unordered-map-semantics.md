---
title: Unordered map semantics
date: 2026-09-15
---

## Status

Accepted

## Context

Slug maps previously exposed insertion order through merge behavior and
`keys(map)`. That makes a storage detail observable and requires every future
map implementation to retain an ordered entry sequence, even when a persistent
hash map would be faster and more memory-efficient. Programs that need a
specific presentation or traversal order can order a key list explicitly.

## Decision

Maps are immutable unordered key/value values.

Map literals, merge operands, and map-copy expressions still evaluate in source
order. That evaluation order is not retained by the resulting map. `keys(map)`
returns every key in unspecified order. Map merge overwrites a matching key and
map copy replaces or adds a string key without any position rule.

Map equality compares key/value membership rather than entry sequence.

## Consequences

### Positive

- The VM may use an unordered persistent map representation without preserving
  insertion order.
- Equality and lookup are independent of entry sequence.
- Programs that require ordering state it explicitly at the point of use.

### Negative

- Programs that relied on `keys(map)` insertion order must order the returned
  list themselves.
- Tests and diagnostics must not infer a semantic order from the current
  vector-backed implementation.

### Neutral

The current representation may continue to produce a stable order internally.
It is not a language guarantee. A future indexed representation remains gated
on a numeric key-equivalence and hashing contract, including `1 == 1.0`.

## Migration

Replace uses that depend on `keys(map)` sequence with an explicit ordering
operation after key enumeration.
