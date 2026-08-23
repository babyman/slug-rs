# Use directional list append and prepend

## Context

The grammar reserved `:+` and `+:`, but the source specification did not say
which operands they accept or whether either operation mutates a list.

## Decision

`list :+ value` appends one value and `value +: list` prepends one value. Each
operation requires its directional list operand to be a list and returns a new
list. Invalid operands produce checked runtime type errors.

## Consequences

Source code can build lists without mutating shared list values. The operators
do not concatenate two arbitrary collections or coerce non-list operands.

## Migration

None.
