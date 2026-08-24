# Adopt a cooperative task runtime

## Context

Structured concurrency needs task handles, nursery ownership, channels, and
`select`, while the current VM stores executable frames, lexical binding cells,
and globals in one synchronous interpreter. Running Slug tasks on arbitrary
host threads would immediately conflict with the current `Rc`-based values and
would expose host-worker behavior as an accidental language guarantee.

## Decision

The runtime will schedule Slug tasks cooperatively on its owning runtime
thread. Each task owns independent VM execution state and a cached settlement,
while sharing only the Slug-visible identities required by the language:
captured values, live outer captures, module globals, channels, and resources.
Nurseries own descendants and their failure accounting; they do not own lexical
bindings or create host threads. A task may yield only at runtime-owned blocking
operations such as channel, timer, task-await, and `select` operations.

The initial task slice may run a task to settlement when it cannot block. The
task representation must nevertheless retain a cached outcome and must not
make eager execution or host-thread count a public compatibility promise.

## Consequences

The VM must distinguish immediate lexical captures, which `spawn` snapshots,
from transitive captures, which remain live. Module globals must be represented
by shared binding cells when tasks receive independent VM state. Future channel
and timer work extends the cooperative scheduler rather than adding a second
native worker queue.

The initial task implementation shares one reference-counted, interior-mutable
global environment between parent and child VMs; lexical binding cells remain
the separate mechanism for capture semantics.

Native callbacks remain synchronous and execute in their calling task. Native
producer capabilities wake runtime-owned work only through channels, as defined
by the native extension interface.

## Migration

None. The task runtime is new and the existing VM bytecode remains private.
