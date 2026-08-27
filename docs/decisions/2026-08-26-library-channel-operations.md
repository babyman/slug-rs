# Make channel operations library bindings

## Context

`slug.channel` already implemented `send` and `recv` in source with `select`
case forms, but the runtime also exposed ambient builtins with the same names.
That duplicated the public API and made channel construction and closure
library-only while sending and receiving remained global.

## Decision

Remove the ambient `send` and `recv` builtins. Keep `send` and `recv` special
only as `select` case headers. The ordinary callable operations are the
exported `slug.channel.send` and `slug.channel.recv` source-library bindings.

## Consequences

The VM owns channel scheduling and checked faults through `select`, while the
standard library owns the callable API, including its timeout behavior. All
ordinary channel operations now share one explicit import boundary.

## Migration

Replace ambient calls with imported bindings:

```slug
val { recv, send } = import("slug.channel")
send(channel, value)
recv(channel)
```
