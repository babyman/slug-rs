# Run stalled interactive submissions as shared-VM tasks

## Context

`Vm::start_named` owns one host-driven root execution at a time. Keeping a
stalled interactive submission in that slot prevents another session from
starting, even though the VM scheduler already owns independent task execution
and native-progress handling.

## Decision

In the default concurrent runtime, an interactive submission starts as a
scheduler-owned task whose closure carries that session's global environment.
The server drives the selected task to its local non-blocking fixed point with
the existing native-ingress, timer, and task machinery. A submission that
cannot progress returns `{ "state": "stalled" }`; `session.poll` drives that
same task again and returns its terminal value or runtime diagnostic once it
settles.

The session manager records `Idle`, `Active`, and `Stalled` execution states.
It rejects a second submission to an active or stalled session, but another
session may start and progress its own task in the shared VM. Closing a session
cancels its active task.

## Consequences

No interactive scheduler or VM-per-session model is introduced. Native
producers still only signal progress; the host calls `session.poll` to resume
the intended session. The VM's single root host-execution slot is therefore
not suitable as the multi-session execution context and remains separate from
these scheduler-owned session tasks.

The slim runtime retains its synchronous submission path for now. Its
host-managed stalled counterpart is the next milestone and does not change the
protocol shape established here.

## Migration

None.
