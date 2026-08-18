# Recover errors by returning nil from the handling function

## Context

`defer onerror(name)` is the language-level recovery mechanism, but the
specification did not define which continuation receives control when its
handler returns normally. The VM must also preserve the distinction between a
user-thrown value and a checked runtime fault at the handler boundary.

## Decision

Compile an error handler as a one-argument closure. During VM unwinding, pass
through the thrown value unchanged or construct a string-keyed fault map with
`type`, `msg`, and `data` fields. A normal handler completion resolves the
active error by completing the handler's enclosing function with `nil`; the
caller resumes normally. Any remaining deferred actions in that function run
as successful cleanup. A new error from a deferred action replaces the active
error and retains it as its cause.

## Consequences

- Error recovery has a predictable function-call result without resuming at an
  invalid instruction inside the failed scope.
- Callers can compose recovered functions using the ordinary `nil` value.
- VM errors retain a structured cause for later diagnostics, while helper
  frames remain absent from Slug-visible traces.

## Migration

None. `defer onerror` was previously unsupported by the Rust source subset.
