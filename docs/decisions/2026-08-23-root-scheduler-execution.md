# Root scheduler execution

## Context

The initial channel implementation retained spawned-task execution but rejected
a blocking root evaluation. That made an ordinary program-level `recv` unable
to coordinate with a spawned sender.

## Decision

Root evaluation receives a scheduler waiter identity. When it suspends, the VM
runs ready owned tasks until the root receives a call result or error, then
continues the same VM state. The scheduler reports a checked blocked-task error
only when neither root nor a child can make progress.

This supersedes the root-blocking limitation in
`2026-08-23-resumable-channel-tasks.md`.

## Consequences

Top-level `send`, `recv`, and `await` may block cooperatively. Root execution
does not require a synthetic public task value. `select` remains unsupported.

## Migration

Programs that previously received a root-blocking runtime error now wait for
available scheduler work or receive the checked blocked-task error.
