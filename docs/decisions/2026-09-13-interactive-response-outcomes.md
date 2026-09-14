# Separate interactive cell outcomes from session state

## Context

Interactive responses previously used one `state` field for both the submitted
cell and the session's retained background work. A completed expression entered
while an earlier cell remained blocked could therefore return `stalled` and
lose its value, even though the submitted expression had completed normally.

## Decision

`submit` responses report the submitted cell as `status: completed`,
`status: stalled`, or `status: incomplete`. Completed responses include their
`value`. Responses also report `session_state: idle | stalled` when they have a
session execution context. `session.poll` reports only `session_state`, because
it has no submitted cell. Structured source and runtime errors remain protocol
failures rather than successful statuses.

The terminal REPL renders only the foreground submission outcome: completed
values are printed, and `[stalled]` appears only when the entered cell stalls.

## Consequences

Interactive clients no longer need to infer whether `stalled` describes their
request or pre-existing session work. Protocol consumers must read `status`
and `session_state` independently. Server and REPL tests must cover a completed
submission while a separate retained cell remains stalled.

## Migration

Clients that read `result.state` must migrate to `result.status` for submit
outcomes and `result.session_state` for retained session work. The protocol
version remains one because the protocol is in-process and unstable.
