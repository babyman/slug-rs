# Attribute all interactive output as protocol events

## Context

The original interactive output sink captured synchronous program stdout only.
The protocol also defines stderr events and needs a safe rule for output from
host-managed shared or background work.

## Decision

Program `print` and `println` use the server-owned sink and produce `stdout`
events for the submission's active session. The server exposes
`Server::emit_output(session, OutputStream, data)` for host and
background-runtime output. It accepts `stdout` and `stderr`, requires an
explicit live session identifier, and queues the resulting event for
`take_events`.

The NDJSON executable drains every queued event before writing the response to
the request it just handled. Embeddings can drain events whenever their own
transport is ready. Output for a closed or unknown session is rejected.

## Consequences

All output is session-attributed and raw program bytes cannot corrupt protocol
stdout. A background worker cannot infer a session from VM state; it must keep
the intended session identifier and use the explicit server API. This
single-threaded server does not itself schedule or flush background work while
waiting for standard input; stalled and task-backed delivery remains the next
runtime milestone.

This decision supersedes the initial output-sink decision's synchronous-only
scope.

## Migration

None.
