# Make task await a library binding

## Context

The runtime exposed `await(task)` as an ambient builtin even though task waits
are fully expressible with the existing `select { await task }` case form. The
same behavior was already implemented by `slug.channel.await`, making the
ambient builtin redundant and inconsistent with channel operations.

## Decision

Remove the ambient `await` builtin. `await` remains special syntax only as a
`select` task-await case header. The ordinary task-join API is the exported
`slug.channel.await` library binding, implemented in Slug source with
`select`.

## Consequences

Task-await scheduling, error propagation, repeated completion observation, and
select behavior remain VM responsibilities. Programs that need a callable
task-join operation import it from `slug.channel`; code that only needs direct
control flow may use `select { await task }`.

## Migration

Replace ambient `await(task)` calls with an imported binding:

```slug
val { await } = import("slug.channel")
await(task)
```
