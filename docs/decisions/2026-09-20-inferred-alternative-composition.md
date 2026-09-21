# Compose inferred overload alternatives by reachable requirements

## Context

Functions can contain several overloaded operations. Their alternatives must
remain correlated across parameters; publishing independent unions would admit
mixed pairs that no execution path accepts.

## Decision

Sequential reachable expressions intersect their relational alternatives.
Reachable branches without a proven type-discriminating guard also intersect
their requirements. Direct `x == nil` and `x != nil` partitions instead retain
separate guarded relations and union those disjoint input domains. Nested
functions infer independently. Defaults distinguish supplied and omitted call
shapes. `recur` eliminates candidates incompatible with its arguments but does
not create candidates or use an unbounded fixed point.

If composition cannot retain a finite correlated set, it widens to dynamic
behavior rather than independent unions.

## Consequences

`fn(left, right) { left + right; left - right }` publishes `(num, num) ->
num`. Later implementation stages must preserve branch, default, and recursive
call-shape boundaries.

## Migration

Known calls may gain valid static intersections or reject pairs that cannot
satisfy every reachable operation.
