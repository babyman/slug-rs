# Route launched-program output as root protocol events

## Context

`slug-server` can host interactive sessions and a launched Slug program. Session
output has an owning session, but a launched program's output belongs to the
root application. Assigning it to an arbitrary session would make service logs
ambiguous and corrupt the protocol's ownership model.

## Decision

Protocol output events carry an explicit origin: `Root` or `Session(session)`.
Root output is broadcast by the server transport; session output remains
attributed to its session. Program stdout and stderr are protocol data, while
server startup, transport, and panic diagnostics remain process stderr.

The first `slug-server app.slug` implementation runs the root program at
startup and emits its output as root events. Concurrent execution of a stalled
root program and interactive requests remains a later execution-lifecycle
extension; this decision deliberately does not introduce a REPL scheduler.

## Consequences

Clients can distinguish application logs from session evaluations without the
VM knowing transport policy. Existing session event consumers retain their
`session` field, while root events omit it and include `source: "root"`.

This clarifies and extends
[the interactive output-event decision](2026-09-12-interactive-output-events.md).

## Migration

Consumers that assumed every event has a `session` field must handle root
events. Protocol version 1 remains experimental.
