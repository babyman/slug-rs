# Detach slim interactive root executions between polls

## Context

The slim runtime has no scheduler tasks, but a single VM root execution can
still suspend on a native channel. Reusing that root slot directly would block
another interactive session from starting and would make the protocol differ
between feature sets.

## Decision

The slim server stores a detached host-driven execution context for each
active session. The context contains the existing VM root state: frames,
operand stack, waiter registrations, native-progress driver, and pending root
execution. Before a `submit` or `session.poll`, the server activates only the
selected context, calls the existing non-blocking `run_until_stalled`, then
detaches it again and restores the shared host globals.

Native producers remain restricted to sending a value and signaling progress.
They do not enter the VM. The host resumes the selected context through
`session.poll`, exactly as in the concurrent build.

## Consequences

Both builds expose `submit`, a `stalled` result, and `session.poll` with the
same observable behavior. The default build uses scheduler-owned tasks; the
slim build uses detached root contexts. Neither model allocates a VM per
session or introduces a REPL-owned scheduler.

This supersedes the task-backed-session decision's statement that the slim
counterpart remained future work.

## Migration

None.
