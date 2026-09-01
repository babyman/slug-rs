# Contextual Stacktrace Rendering

## Context

The previous Slug implementation exposed `stacktrace(error)` to render an
error while it was being handled. A stacktrace must help diagnose replacement
failures during unwinding without making source-file availability, host
embedding, or a general-purpose retained error-object API part of its contract.

## Decision

`stacktrace(error)` takes one required argument and accepts only the active
runtime error, including the value currently bound by `defer onerror(err)`.
Passing any unrelated value is a runtime error.

It returns a human-readable string. The rendering includes the active error's
message, its available primary path/line/column, and its Slug call frames. A
frame includes its call-site path/line/column when available. Thrown values
remain visible in the message, for example `uncaught throw: boom!`.

When an error replaces another during unwinding, the rendering recursively
includes the prior error under `caused by:`. It follows every available cause
back to the oldest reachable runtime error. Implementations must defensively
avoid an infinite rendering loop if a cause cycle is encountered.

The function does not render source excerpts or read source files. Coordinates
are sufficient in an in-language string and work when source text has been
removed or was never file-backed. The CLI remains responsible for best-effort
source excerpts and carets.

## Consequences

Runtime-error construction must retain the preceding active runtime error for
all checked replacement paths where one exists, including failures raised by
deferred cleanup. The stacktrace renderer must preserve each error's own
location and frames instead of collapsing the causal chain.

The result is explicitly presentation-oriented and is not a stable,
machine-readable error-data format. A later structured inspection API, if
needed, must be separate from `stacktrace`.

## Migration

None. This records the intended behavior before implementation.
