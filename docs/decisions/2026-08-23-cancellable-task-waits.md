# Cancellable task waits

## Context

Channels and task await retain parked tasks in FIFO wait queues. Logical
nursery cancellation must not leave a cancelled task visible to later send,
receive, or task-completion operations.

## Decision

Each suspended task owns one internal wait registration: channel send, channel
receive, or task await. Resumption and ordinary settlement clear the
registration. Cancellation removes the task from the referenced wait queue
before publishing its cancellation outcome and waking tasks that await it.

## Consequences

Fail-fast nurseries safely cancel parked tasks without allowing stale channel
messages or stale await wakeups. This single-registration model is the base for
future `select`, which will require a task to manage several registrations and
remove losing cases atomically.

## Migration

None.
