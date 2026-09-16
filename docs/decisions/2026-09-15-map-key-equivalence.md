---
title: Map-key equivalence
date: 2026-09-15
---

## Status

Accepted

## Context

Maps are unordered and may later use an indexed representation. Slug currently
represents integers and binary floats separately, so a host-language hash or
an lossy integer-to-float conversion could make equal keys hash differently or
make distinct large integers compare equal.

## Decision

Map keys are booleans, numbers, strings, or bytes. Keys use the same equality
as Slug values, with these numeric rules:

- An integer equals a float only when the float is finite, integral, within the
  `i64` range, and converts back to that identical integer. Thus `1 == 1.0`.
- `0` and `-0.0` are equal.
- Infinities never equal an integer, but equal the same-sign infinity.
- NaN is unequal to every value, including itself, and is not a valid map key.

An internal map key canonicalizes an exactly representable integral float,
including signed zero, to its integer form. Other finite floats and infinities
use their canonical IEEE bit representation. Equal keys must have identical
canonical forms and hashes.

## Consequences

### Positive

- A future map index can preserve `1`/`1.0` lookup, merge, removal, and
  pattern behavior without precision loss above `2^53`.
- Signed zero and non-finite values have explicit, portable behavior.

### Negative

- Native callers cannot use NaN as a map key, even though they can carry NaN
  as an ordinary numeric value.

### Neutral

This clarifies the numeric portion of the current `i64`/`f64` representation;
it does not select the long-term numeric representation described in
`docs/planning/numeric-representation-decision.md`.

## Migration

No source literal denotes NaN or infinity. Native code that attempted to use
NaN as a map key now receives the ordinary invalid-key failure.
