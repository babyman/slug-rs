# Preserve thrown Slug values at the VM boundary

## Context

Slug permits `throw` to propagate any Slug value. The initial VM reported only
host-created runtime faults, so treating a source `throw` as a formatted string
would discard the value needed by later `defer onerror` recovery and make its
diagnostics inconsistent with ordinary faults.

## Decision

Add an internal `Op::Throw` instruction. It consumes one stack value and ends
the current evaluation with a checked `RuntimeErrorKind::Thrown`. The error
retains the original Slug value, the instruction's source span, and the active
Slug frames. Its display message is diagnostic text only; the retained value is
the semantic payload.

This first slice implements uncaught throws. Deferred cleanup and recovery will
be added through VM-managed unwinding continuations, so they run for returns,
throws, and runtime faults rather than being compiled as normal-exit-only code.

## Consequences

- Source and bytecode callers can distinguish a user-thrown value from a VM
  fault without parsing an error message.
- Nested calls retain the existing source-located frame diagnostics.
- `defer` remains specified but unsupported until the VM can execute cleanup
  closures during unwinding without exposing helper frames.

## Migration

None. `throw` was previously rejected as an unknown name or expression.
