# Represent native Clutches with library descriptors

## Context

The initial prototype ABI treated a loaded native library as one Slug module:
`slug_ffi_module_init` returned one module descriptor with one module name.
That contradicted the Clutch model, where a native library may implement any
subset of a clutch's modules. SQLite prepared statements need both
`slug.db.sqlite` and `slug.db.sqlite.statement` to be served by the same native
implementation while sharing compatible resource handles.

## Decision

Prototype ABI minor 9 replaces the module initializer with
`slug_ffi_library_init`, which returns a library descriptor containing zero or
more module descriptors. The library descriptor owns the dynamic-library lease,
optional library teardown callback, and its complete module table. Each module
descriptor continues to own its functions and resource declarations.

All modules exposed by a library share one native resource scope. This lets a
function in one module accept a typed resource created by a sibling module,
without making identically named resources from separate libraries compatible.

## Consequences

This is an intentional breaking change to the experimental ABI. Native code no
longer exports `slug_ffi_module_init`, and module-level teardown is not used for
library-owned state. A single initialized native library can now register
multiple ordinary Slug modules, while its resources still close before its
library lease is released.

## Migration

All native clutch adapters and C test fixtures export a library descriptor and
select `slug-ffi-prototype/0.9`. The host does not support the old initializer.
