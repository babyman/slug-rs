# Use directional string repetition

## Context

The arithmetic grammar reserves `*`, but string repetition needs a defined
operand order and safe behavior for invalid or excessive counts.

## Decision

`string * count` repeats the left string `count` times. The count must be a
non-negative integer. The reverse form is not supported. Negative,
non-integer, and unrepresentably large repetitions are checked runtime type
errors.

## Consequences

String repetition is clear at the call site and never coerces numeric values.
The VM checks the result size and allocation before constructing the result.

## Migration

None.
