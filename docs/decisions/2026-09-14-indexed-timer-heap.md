# Use an indexed timer heap

## Context

The scheduler's vector-backed timer service scanned every pending timer to find
the next deadline, wake due timers, and remove losing `select` registrations.
The scaled timer benchmark demonstrated quadratic scan growth: 128 timers
examined 219,858 wakeup entries across 25 runs. This supersedes [Retain
measured scheduler queues](2026-08-30-retain-measured-scheduler-queues.md)'s
provisional vector-backed timer decision.

## Decision

Each nursery uses a binary min-heap ordered by deadline and registration order.
An index maps a private timer ID to its heap position. A timer wait registration
retains that ID, allowing winner-removes-losers and cancellation to remove an
exact timer in O(log n). The next deadline is the heap root, and due timers are
popped from that root.

Equal deadlines retain source registration order. FIFO ready queues and channel
waiter queues are unchanged.

## Consequences

Timer registration, wake-up, and cancellation now trade a small position-map
allocation and heap maintenance for bounded lookup work. The opt-in metrics
continue to report timer work, but deadline and wakeup entry counters now count
root accesses and popped due timers rather than vector scans; timer-removal
work counts indexed registration lookups.

Scheduler tests must continue to cover due timers, losing-select cleanup,
task cancellation, and host-driven progress. The benchmark must retain scaled
timer workloads so future representation changes can be measured.

## Migration

None for Slug source, bytecode, or compiled artifacts.
