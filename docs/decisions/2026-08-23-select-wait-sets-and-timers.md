# Select wait sets and timers

## Context

A blocked `select` can wait on several channels, task handles, and timers at
once. Leaving the losing registrations in place can consume a later message or
resume an already continuing evaluation. The cooperative scheduler also needs
a monotonic source of progress when no task is ready but a timer is pending.

## Decision

Each blocked select owns one wait set shared by all of its case registrations.
The first case that resumes the select marks it selected and removes the whole
wait set before delivering the case value and optional handler to the parked
execution. Task cancellation removes the same registrations.

Each nursery owns a monotonic timer queue. `after N` accepts a non-negative
integer number of milliseconds and resumes with `nil` after its deadline. When
the ready queue is empty, the scheduler waits only until the earliest pending
timer deadline; otherwise it reports the established checked blocked-task
error.

## Consequences

Channel and task completion cannot be consumed by a losing select case. Timer
progress uses the existing cooperative scheduler rather than host worker
threads. Tie-breaking remains intentionally unspecified; source-order scans
may determine a particular implementation's ready-case choice.

## Migration

None. `select` was previously rejected during source compilation.
