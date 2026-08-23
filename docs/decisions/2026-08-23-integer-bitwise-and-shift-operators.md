# Keep bitwise and shift operations integer-only

## Context

The grammar specifies bitwise and shift operators but did not define their
numeric domain, right-shift behavior, or invalid shift counts.

## Decision

Bitwise and shift operations accept only signed 64-bit integers. Shift counts
are valid from zero through 63. Right shifts are arithmetic, preserving a
negative sign. Invalid operands and invalid counts are checked runtime type
errors.

## Consequences

The operations never coerce floating-point values or rely on host panic or
implementation-defined shift behavior.

## Migration

None.
