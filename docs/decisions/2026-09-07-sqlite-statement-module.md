# Share SQLite statement resources through one Clutch

## Context

The SQLite Clutch proved direct variadic execution but not that a single native
implementation can serve multiple source modules or own related resources.

## Decision

Add `slug.db.sqlite.statement` to the existing Clutch. It exposes the
Clutch-native prepare, execute, query, and close functions as a focused source
module. The shared native implementation owns both `Database` and `Statement`;
statement destruction calls `sqlite3_finalize` and database destruction uses
SQLite's deferred close operation.

## Consequences

The two imports resolve through one Clutch and one native library lease.
Statements are reusable after reset, but a closed resource is rejected by the
native resource boundary. Shutdown finalizes live statements before releasing
database resources and plugin state.

## Migration

None.
