# Use the error-handler result for recovery

## Context

[Recover errors by returning nil from the handling function](2026-08-17-defer-onerror-recovery.md)
selected `nil` as the result of a recovered function. That discards a handler's
computed fallback value and makes recovery less composable than ordinary
function control flow.

## Decision

This supersedes the prior recovery-result decision. When a `defer onerror`
handler returns normally, its returned Slug value becomes the result of its
enclosing function. The caller resumes with that value after the enclosing
function's remaining successful cleanup actions complete.

## Consequences

- Handlers can provide typed or computed fallback values without mutating
  outer state.
- A handler that ends with no explicit value still returns `nil`, following
  ordinary block evaluation.
- Error replacement through an explicit `throw` is unchanged.

## Migration

Programs using a normal `onerror` handler now receive the handler's value
instead of `nil` from the enclosing function.
