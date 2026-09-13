# Retain suspended interactive forms as background cells

## Context

An interactive session previously retained one active submission. A channel
receive that suspended therefore rejected every later source submission with
`submission_active`, including the send that could resume the receive. Session
compiler snapshots commit only after successful execution, so simply allowing
arbitrary concurrent submissions would also make declaration visibility and
commit order ambiguous.

## Decision

The concurrent interactive server compiles a complete submission into ordered
top-level forms and executes them as individual cells. A completed cell commits
its bindings immediately. A suspended cell remains a session-owned background
task, while the server accepts later binding-free cells and pumps every
background task after each request. A suspended cell that declares bindings
commits them only after it settles; while such work remains, later declarations
are rejected to preserve compiler-state order.

## Consequences

Channel setup can complete before a later receive blocks, allowing a later
interactive send to wake it. Output produced by a resumed cell remains
attributed to its owning session. The server tracks multiple retained tasks per
session and cancels all of them when the session closes. Background failures do
not yet have a dedicated unsolicited diagnostic event.

## Migration

Interactive sessions that previously received `submission_active` after a
stalled receive may now submit binding-free source. A declaration submitted
while a background form remains pending receives `background_bindings` until
the pending work settles.
