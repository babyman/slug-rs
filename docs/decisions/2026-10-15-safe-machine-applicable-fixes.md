# Diagnostics offer only fixes that cannot change program meaning

## Status

Accepted

## Context

A machine-applicable fix lets an agent or editor repair source without
interpreting a message. An applied fix that changes what the program means
is worse than none: it turns a reported error into a silent behavior change.
Many errors have a likely repair that is sometimes wrong, such as changing
`val` to `var` for a reassigned binding when the author meant a new binding.
See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

A diagnostic offers a fix only when applying it cannot change the meaning of
the rest of the program. Likely but uncertain repairs are not offered.

- At the end of input, a missing closing delimiter is inserted after the last
  token. Everything after the opening delimiter was already parsed as inside
  it, so closing it there completes the program as parsed.
- An unreachable match case is deleted when testing its patterns evaluates
  nothing. A case can be tested and fail without ever being selected, and a
  pinned name or computed map key evaluates code when tested, such as a read
  of an uninitialized binding that raises an error.
- Fixes are edits against the reported source, defined in
  [Diagnostic Documents](../reference/diagnostics.md#fixes). Tests apply every
  offered fix and check that the program then compiles or exposes the next
  error.

## Consequences

### Positive

- An applied fix never silently changes behavior.

### Negative

- Most errors have no fix, including ones with an obvious likely repair.

### Neutral

Each new fix needs its own argument that it cannot change meaning, recorded
with it in the reference.

## Migration

None.
