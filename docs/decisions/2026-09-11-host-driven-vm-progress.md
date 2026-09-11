# Host-driven VM progress

## Context

Native channel producers may publish from threads or callbacks that cannot own
or re-enter the Slug interpreter. The scheduler previously combined generic
native wake notification, available runtime work, and condition-variable
waiting, making a blocking command-line policy appear to be a VM requirement.

## Decision

The VM exposes host-driven progress through `Vm::poll` and
`Vm::run_until_stalled`. They drain currently available ingress, runnable work,
and due timers, but never wait for an operating-system event. A native producer
emits only a generation-counted `ProgressSignal`; it does not identify or
resume a Slug task.

`Vm::blocking_run` is the blocking adapter used by the existing `run` APIs.
Its private waiter owns the condition-variable wait and retries host-driven
progress after a notification or timer deadline.

## Consequences

Embedders can drive one VM owner from an event loop without interpreter
re-entry. Existing blocking source execution and the native producer ABI are
unchanged. The default-enabled `concurrency` Cargo feature supplies
scheduler-owned tasks, timers, nurseries, and task-aware `select`; a slim
`--no-default-features` build retains channels, ingress, and host-driven
progress. The source language remains one model: an executed scheduler-only
operation in the slim runtime reports a checked unavailable-capability error.

## Migration

None. Existing `run`, `run_named`, and command-line execution retain blocking
behavior. Hosts that need event-loop control may start an entry with `start` or
`start_named` and drive it with the new progress methods.
