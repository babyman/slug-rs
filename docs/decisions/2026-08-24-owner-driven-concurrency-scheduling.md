# Owner-driven concurrency scheduling

## Context

Root evaluation, explicit nursery bodies, spawned tasks, timers, and native
producer mailboxes all participate in one cooperative concurrency model. The
initial implementation gave only root evaluation and spawned tasks resumable
scheduler identities. It also polled native mailboxes for a fixed interval and
returned immediately from owner settlement when the owner body failed.

Those choices allowed valid nursery bodies to reject blocking operations,
allowed an error path to abandon pending children, and made native progress
depend on whether an event arrived inside the polling interval. A timer sleep
could also prevent an earlier native event from being observed.

## Decision

Every concurrency owner drives its body and children through the same resumable
task scheduler. An explicit nursery body is an internal scheduler-owned task;
it is not exposed as a Slug task value and does not count against the nursery's
direct-child limit.

Owner settlement runs for both successful and failed body outcomes. An
explicit nursery cancels pending children when its body fails. The root owner
continues joining runnable descendants after a root failure and cancels any
descendants that cannot make further progress before returning the original
failure. Settlement must leave no pending execution retained by the owner.

Native producer mailboxes notify each nursery scheduler that is currently
waiting on their receiver. The scheduler arbitrates ready tasks, mailbox
notifications, and the nearest monotonic timer deadline through one wait path.
It reports a blocked-task error only when no runnable work, timer, or live
native producer can make progress.

This supersedes the fixed native polling consequence in
`2026-08-23-native-producer-mailboxes.md` and extends the scheduler ownership
adopted by `2026-08-23-root-scheduler-execution.md` to explicit nursery bodies.

## Consequences

Nursery bodies may use `send`, `recv`, `await`, timers, and `select` without a
special blocking restriction. Error paths cannot leave task handles pending on
an inaccessible ready queue. Native events are not rejected merely because
they arrive after an arbitrary polling duration, and timers do not mask an
earlier mailbox event.

The runtime needs an internal scheduler notification primitive and must
register native channels with every nursery that waits on them. Internal
nursery-body tasks add one orchestration state that tests and diagnostics must
keep private.

## Migration

Programs that previously received `blocking operations require scheduler-owned
execution` from a nursery body now suspend cooperatively. Timing-dependent
blocked-task errors from live native producers are removed. No source syntax or
private bytecode changes are required.
