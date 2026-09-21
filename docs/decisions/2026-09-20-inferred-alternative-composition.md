# Compose inferred overload alternatives by reachable requirements

## Context

Functions can contain several overloaded operations. Their alternatives must
remain correlated across parameters; publishing independent unions would admit
mixed pairs that no execution path accepts.

## Decision

Sequential reachable expressions intersect their relational alternatives.
Reachable branches without a proven type-discriminating guard also intersect
their requirements. Direct `x == nil` and `x != nil` partitions instead infer
their inputs and results independently under the narrowed branch environment,
then union only their proven-disjoint guarded relations. A guarded relation
uses an `AlternativeType` result expression as well as `AlternativeType`
inputs: `if (left == nil) { right } else { left + right }` may retain
`(nil, T) -> T`, rather than widening the nil branch result to `unknown`.
If a guarded branch result cannot be expressed as a finite correlated relation,
that partition is omitted and the call remains dynamic. Nested functions infer
independently. Defaults distinguish supplied and omitted call shapes. `recur`
eliminates candidates incompatible with its arguments but does not create
candidates or use an unbounded fixed point.

If composition cannot retain a finite correlated set, it widens to dynamic
behavior rather than independent unions.

## Consequences

`fn(left, right) { left + right; left - right }` publishes `(num, num) ->
num`. Later implementation stages must preserve branch, default, and recursive
call-shape boundaries.

## Migration

Known calls may gain valid static intersections or reject pairs that cannot
satisfy every reachable operation.
