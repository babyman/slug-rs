# Add a broad source `resource` type

## Context

An opaque native resource is meaningful in a public foreign signature. Leaving
the result of `openRead(path)` unannotated makes a file-opening API appear to
return an unconstrained value, even though the result must be passed to
resource-aware operations and explicitly closed.

The lifecycle decision deferred both a broad source category and nominal
resource syntax. The filesystem API now supplies a concrete need for the broad
category, while there is still only one resource library and no demonstrated
need for nominal source identity.

## Decision

Slug has a built-in, non-parameterized `resource` type. It denotes any opaque
native resource handle. Foreign declarations may use it in parameters and
results; for example:

```slug
export foreign openRead = fn(path:str):resource
```

`resource` is runtime-checkable in a whole-value match constraint. It does not
reveal a resource's module or native kind. A native operation must continue to
validate the resource registration and open state itself.

This supersedes the broad-category deferral in
[Keep resource lifecycle explicit and defer nominal resource syntax](2026-09-03-resource-lifecycle-and-typing.md).
It does not adopt opaque type declarations, qualified resource names, or
`resource<T>`.

## Consequences

Foreign APIs can honestly advertise an opaque-handle return value, and static
checking rejects an ordinary value where a declared resource is required.
Native resource ownership and lifecycle stay unchanged: `defer close(handle)`
remains the idiomatic release mechanism.

The broad type cannot prevent crossing resource kinds. For example, a future
database resource also conforms to `resource`; its operations must reject a
file handle. Nominal resource typing remains a future design only when several
libraries show the need and its namespace/import rules can be specified.

## Migration

Existing unannotated foreign declarations remain valid. Libraries may add
`:resource` results and `resource` parameters to expose their existing runtime
contract more precisely.
