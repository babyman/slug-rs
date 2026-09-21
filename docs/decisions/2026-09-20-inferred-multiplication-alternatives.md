# Infer multiplication alternatives from function bodies

## Context

Multiplication supports numeric arithmetic and string repetition. Their input
and result relationships are finite and useful at known call sites.

## Decision

`*` derives private alternatives `(num, num) -> num` and `(str, num) -> str`.
The `num` count type does not establish integrality or non-negativity; the VM
continues to check string repetition counts and allocation size at runtime.

## Consequences

Known mixed operand pairs are rejected statically, while negative or fractional
string repetition counts remain checked runtime failures. No specialization or
source syntax is added.

## Migration

Known calls may reject pairs outside the two published alternatives.
