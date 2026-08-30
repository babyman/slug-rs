# Retain measured scheduler queues

## Context

The scheduler uses FIFO ready queues and a vector-backed timer service.
Replacing either structure would add cancellation and fairness complexity, so
the choice requires workload evidence rather than an assumed asymptotic gain.

## Decision

Keep the existing FIFO ready queues and vector-backed timer service. The
default-disabled `metrics` feature now records timer registrations, deadline
scans, timer wakeups, and wait-registration removals. The VM benchmark covers
32 concurrent timers, a 16-case timed select, and fail-fast cleanup of
suspended multi-wait tasks.

Reconsider a cancellation-safe indexed timer structure only when a supported
workload shows these queues are a material cost. Any replacement must preserve
FIFO channel arbitration and winner-removes-losers behavior.

## Consequences

The current implementation stays small and its cancellation behavior remains
covered by focused VM tests. Scheduler counters are opt-in and do not affect
ordinary VM builds. Future queue work has a concrete corpus and counter set
with which to justify a change.

## Migration

None for Slug programs, private bytecode, or the future compiled-artifact
format.
