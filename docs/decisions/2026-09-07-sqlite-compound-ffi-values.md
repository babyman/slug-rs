# Transfer compound query values through the prototype FFI

## Context

The existing native-clutch prototype could borrow scalar arguments and return
scalar values or opaque resources. It could not prove that an ordinary native
library can bind Slug values from a variadic call and construct a query result
without inspecting VM-owned list or map storage.

## Decision

Extend prototype ABI minor 8 with call-scoped byte and scalar-kind argument
operations plus opaque list and map builders. Builders accept only string map
keys and scalar values in this experiment; maps transfer into lists and the
final list transfers into the call result. Adopt `slug.db.sqlite` as an
exploded clutch with a nominal `Database` resource and variadic `exec` and
`query` functions. It maps SQLite's NULL, integer, real, text, and blob values
to Slug nil, num, str, and bytes.

## Consequences

The C adapter uses only the public prototype table for values and does not
depend on `Value`, reference counts, or collection layout. The API remains
version-0 and intentionally does not provide a general persistent Slug-value
root, arbitrary map keys, prepared statements, transactions, pooling, or
migrations. Resource destruction continues to close SQLite handles during
explicit close and deterministic VM shutdown.

## Migration

Native clutch manifests in this checkout select `slug-ffi-prototype/0.8`.
