# Run plain deferred actions through VM cleanup frames

## Context

Deferred source expressions must retain lexical captures and run after their
enclosing scope exits. Emitting their calls inline would run only on the normal
path and would miss returns and throws.

## Decision

Compile each plain `defer expression` as a zero-argument closure and register
it in the active VM scope. The VM drains scope actions in last-in, first-out
order through internal cleanup frames before completing a normal return or an
uncaught throw. Cleanup-frame results are discarded.

## Consequences

- Deferred closures retain the bindings visible at their declaration.
- Plain cleanup is shared by block exits, function returns, and uncaught
  throws without exposing helper frames in the original thrown error.
- `onsuccess`, `onerror` recovery, and runtime-fault cleanup require further
  unwinding work and remain unsupported.

## Migration

None. Plain `defer` was previously rejected by the source frontend.
