# Resumable channel tasks

## Context

Channel receive and bounded send can block after entering arbitrary calls and
deferred-cleanup scopes. Restarting a child task would duplicate work and lose
its stack, lexical state, and cleanup obligations.

## Decision

Task execution returns an internal suspended outcome that retains the owned VM
and program. Channel wait queues retain tasks, resume their saved call result,
and requeue them in the owning nursery's FIFO ready queue. The initial public
surface is `channel(capacity)`, `send`, `recv`, and `close`; zero capacity is a
rendezvous channel and positive capacity is bounded FIFO buffering. Root
evaluations reject blocking operations until they also gain scheduler-owned
execution objects.

## Consequences

Blocked tasks keep their frames and deferred actions alive. A resumed channel
error follows the usual VM cleanup path. `select`, root-task suspension, and
fairness beyond FIFO per queue remain intentionally unsupported.

## Migration

None.
