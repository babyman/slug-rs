# Execute cross-program closures in resumable frames

## Context

Interactive submissions and imported modules create closures whose bytecode
belongs to a program other than the caller's current program. The VM currently
answers such a call by constructing a nested VM and running it synchronously.
A channel or select suspension inside that nested VM is therefore converted to
a blocked-task runtime error instead of suspending the caller's task.

## Decision

Program identity belongs to each execution frame, not to a task or VM
execution as a whole. Calling a Slug closure always pushes a frame for the
closure's program onto the existing resumable execution. It MUST NOT construct
a nested synchronous VM merely because the closure originated in another
program.

Interactive sessions remain host-owned supervisors of ordinary submitted-cell
tasks. They share committed bindings and compiler state, but do not introduce a
distinct Slug task kind.

## Consequences

Imported-library functions and closures retained from earlier interactive cells
can suspend on the caller's task. Instruction dispatch, source spans, cleanup,
and calls must resolve their program from the active frame. The old
single-program execution assumption and nested module-closure runner are
removed.

## Migration

Programs that previously reported a blocked-task error when a cross-program
closure waited now suspend and resume through the ordinary scheduler.

## Implementation notes

Frames also retain the closure's lexical global environment. This is required
for imported functions to resolve module-private bindings after dispatch moves
to their frame. Interactive cells execute against private binding overlays;
the host promotes their declared bindings only after successful settlement.
