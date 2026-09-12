# Route interactive output through a server-owned sink

## Context

The interactive server uses stdout for NDJSON protocol traffic. Reusing the
CLI's process-stdout `print` and `println` host bindings would corrupt that
transport and lose the session that produced output.

## Decision

`slug-server` registers server-owned native `print` and `println` bindings.
During a submission, they append `stdout` records to a server-owned sink tagged
with the active session. The binary drains those records as NDJSON events before
writing that submission's response.

The bindings are ordinary server host natives, not `slug.builtin` descriptors;
the latter are reserved for the `slug.builtin` module contract.

## Consequences

Program output cannot corrupt protocol stdout and clients can associate output
with its session. This first sink is synchronous and submission-scoped.
Background-work ordering and stderr semantics remain Milestone 4 work.

## Migration

None.
