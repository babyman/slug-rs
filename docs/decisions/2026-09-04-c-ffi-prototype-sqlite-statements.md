# Make SQLite statement ownership explicit

## Context

An SQLite database handle alone cannot expose whether the resource model safely
handles child allocations. Prepared statements retain database state and make
an explicit parent close fail while final cleanup must remain safe in either
drop order.

## Decision

The SQLite fixture declares `Statement` as a second opaque resource.
It exposes prepare, integer binding, single-row integer stepping, and explicit
statement close. An explicit `close(database)` first calls `sqlite3_close` and
returns `sqlite.error` when active statements make the close busy. Final
database destruction uses `sqlite3_close_v2`; statement destruction finalizes
the SQLite statement. This keeps error unwinding and registry teardown safe
without adding parent-child edges to the general host resource API.

## Consequences

- The FFI experiment validates a real parent/child lifetime constraint.
- SQLite, rather than the host, owns the close-busy decision for this adapter.
- General parent/child resource tracking, row iteration, bindings beyond
  integers, and statement reset/reuse remain unimplemented.

## Migration

The resource-table names are the exact public names declared by the companion
Slug module: `Database` and `Statement`. There is no separate internal-to-Slug
resource-name mapping.
