# Infer subtraction alternatives from function bodies

## Context

Subtraction has two runtime behaviors: numeric subtraction and persistent map
key removal. An unannotated function body containing `left - right` must retain
those relationships without treating independently widened parameter unions as
correlated input pairs.

## Decision

`-` may derive these private inferred alternatives:

```text
(num, num)                            -> num
(map<K,V>, bool|num|str|bytes)        -> map<K,V>
```

The map-removal operand is constrained by the hashable map-key domain, not by
the map's `K`. Removing an absent key is valid, so `map<num,str> - "missing"`
is a valid `map<num,str>` expression. A statically known union is accepted
only when every member belongs to that domain. `unknown`, `any`, and unions
containing either retain ordinary dynamic behavior; known `nil`, collections,
functions, and structs do not select this alternative.

The hashable-key domain exists only in private semantic schemes. It introduces
no source annotation or runtime contract. Operator-family composition remains
deferred: a body that directly establishes both `+` and `-` relationships does
not publish an inferred alternative set until a composition rule is adopted.

## Consequences

- Known direct and imported calls receive precise numeric and map-removal
  results, and reject known mixed or unhashable operand pairs.
- Map-removal alternatives retain the map's original key and value types.
- Dynamic calls continue to execute the generic VM operation and report
  checked runtime errors for unhashable keys.

## Migration

Known calls to a function whose body is `left - right` may now be rejected
when neither published alternative accepts the operands. No source syntax or
runtime behavior changes.
