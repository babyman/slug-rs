# Route VM faults through deferred cleanup

## Context

The initial plain-`defer` implementation cleaned up normal returns and source
throws, but ordinary VM faults returned directly from opcode execution. That
made cleanup depend on how an error originated.

## Decision

The VM's outer execution loop now routes every checked execution error through
the same deferred-cleanup dispatcher used for `throw`. The original runtime
error remains the reported failure after cleanup completes.

This supersedes the runtime-fault limitation in
[VM defer cleanup](2026-08-17-vm-defer-cleanup.md).

## Consequences

- Plain deferred actions run before checked type, name, call, collection, and
  arithmetic faults reach the caller.
- VM operations can continue to return `RuntimeError` directly; the execution
  boundary owns the common unwinding behavior.
- Conditional deferred actions and `onerror` recovery remain unsupported.

## Migration

Programs with plain `defer` now observe cleanup before a runtime fault exits.
