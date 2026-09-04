# Give C threads owned producer capabilities

## Context

Resource handles prove synchronous pointer ownership, but do not test whether a
C integration can complete later on a foreign thread without exposing the VM or
retaining arbitrary Slug values.

This narrows the asynchronous-use exclusion in
[`2026-09-03-c-ffi-prototype-opaque-resources.md`](2026-09-03-c-ffi-prototype-opaque-resources.md)
to one explicitly owned producer capability.

## Decision

The prototype provides opaque channel and producer handles. During a callback,
C may create a channel, start its background work, then transfer the channel
receiver to Slug while retaining the paired producer. The producer supports
only non-blocking integer sends and explicit destruction. Its C owner must
destroy it exactly once after all sends. No C API exposes a VM pointer, a Slug
value, native-to-Slug callbacks, or arbitrary cross-thread invocation.

## Consequences

- A C worker can wake a Slug receiver through the existing thread-safe native
  producer path.
- Failure to release a producer is an integration leak; the prototype does not
  yet supervise foreign threads or revoke their capabilities.
- Rich values, retry ownership after a full send, C-owned resources containing
  producers, and cancellation protocols remain later experiments.

## Migration

The unstable prototype minor version is now 3. Existing C module descriptors
remain source-compatible when rebuilt against the header because new host-table
entries append to the table.
