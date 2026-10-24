# A failed owner body cancels its tasks

## Status

Accepted

## Supersedes

[Owner-driven concurrency scheduling](2026-08-24-owner-driven-concurrency-scheduling-superseded.md).

## Context

The superseded record made the root owner keep joining runnable descendants
after its body failed, and report the failure only once they settled. An
explicit nursery instead cancelled its pending children.

A task waiting on a channel that a live native producer feeds, such as a task
in `slug.net.tcp`'s `accept`, can always make progress. So when `main` threw
while a server task was accepting, the program never exited and never printed
the error. It was found when a benchmark client in the same program as an
`http.serve` task threw. A task waiting on a timer likewise delayed the error
until the timer fired, and its code still ran after `main` had failed.

Nothing can await a failed body's children, so running them on only delays
the failure and runs work whose result is discarded.

## Decision

Every concurrency owner drives its body and children through the same
resumable task scheduler. An explicit nursery body is an internal
scheduler-owned task; it is not exposed as a Slug task value and does not
count against the nursery's direct-child limit.

Owner settlement runs for both successful and failed body outcomes. When the
body fails, the root and an explicit nursery alike cancel every pending child
and return the body's failure. When the body succeeds, an explicit nursery
cancels its remaining children at its first unobserved child failure, and
the root joins all its descendants, then reports the first unobserved child
failure. Settlement must leave no pending execution retained by the owner.

Native producer mailboxes notify each nursery scheduler that is currently
waiting on their receiver. The scheduler arbitrates ready tasks, mailbox
notifications, and the nearest monotonic timer deadline through one wait path.
It reports a blocked-task error only when no runnable work, timer, or live
native producer can make progress.

## Consequences

A failed program reports its error as soon as `main` fails, whatever its
tasks are waiting on. A successful `main` still waits for its tasks, so a
program can return from `main` and leave a server task running. Cancellation
settles a task with an error but does not interrupt a host thread.

Nursery bodies may use `send`, `recv`, `await`, timers, and `select` without a
special blocking restriction. Error paths cannot leave task handles pending on
an inaccessible ready queue. Native events are not rejected merely because
they arrive after an arbitrary polling duration, and timers do not mask an
earlier mailbox event.

## Migration

A program whose `main` fails no longer runs its pending tasks to completion
first. Code that relied on a task finishing after `main` threw must await it
before throwing, or catch the error in `main`.
