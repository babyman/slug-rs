# Typed native length results and shared handles

## Context

[Step 2 of the typed native-call plan](../planning/typed-native-calls-step-2.md)
moves `slug.db.sqlite` to raw SQLite functions. Three things it needs are
outside [Typed native manifest bindings](2026-10-11-typed-native-manifest-bindings.md):

- **Results with a length.** SQLite text columns may contain NUL bytes, and
  blob columns are binary. A `cstr` result ends at its first NUL, and there is
  no `str` or `bytes` result.
- **Handles from another module.** `slug.db.sqlite.statement.prepare` takes
  a `slug.db.sqlite.Database`. A signature may name only a resource of its own
  module. The adapter also installs each module in its own native scope, and
  the VM accepts a resource argument only from the declaring function's scope.
- **C functions that cannot be bound directly.** Binding text needs the
  `SQLITE_TRANSIENT` destructor constant, SQLite lengths are `int`, and text
  columns are `const unsigned char *`.

## Decision

**Length-delimited out results.** A parameter may be `out str` or `out bytes`.
Each passes two C parameters, a pointer to the data pointer and a pointer to a
`size_t` length:

| Manifest | C parameters |
|---|---|
| `out str` | `const char **`, `size_t *` |
| `out str owned(free_fn)` | `char **`, `size_t *` |
| `out bytes` | `const uint8_t **`, `size_t *` |
| `out bytes owned(free_fn)` | `uint8_t **`, `size_t *` |

Each is one result cell carrying the pointer and the length the callee wrote.
Ownership works as it does for `cstr`, with the same `void (void *)` free
function. A NULL pointer becomes `nil`, whatever the length. Otherwise the
adapter copies exactly `length` bytes. A `str` must be valid UTF-8 and may
contain NUL. A `str` or `bytes` return value stays unsupported, because a C
function returns one value. `str` and `bytes` arguments are unchanged. The
canonical spellings are `out str`, `out bytes`, and `out str owned(free_fn)`.

**Handles shared within one clutch.** A signature may name
`handle<module.R>` for a resource that another module of the same clutch
declares under `[native.resources."module"]`. An unqualified `handle<R>` still
names a resource of the binding's own module. The canonical text spells a
cross-module handle with its module, as in
`handle<slug.db.sqlite.Database:sqlite3>`, and a same-module handle as before.

The adapter installs all modules of one shim table in one native resource
scope, as the 0.13 prototype loader does for one library, so the VM's resource
identity checks accept a handle passed between them. The scope id comes from
the VM's own module-id counter, through a hidden `slug-vm` constructor for
shared-scope modules, so it cannot equal an unrelated module's id.

Type compatibility accepts, for a cross-module `handle<module.R>`, an
annotation whose last segment is `R` and that names it through an import, such
as `sqlite.Database`. The VM's run-time resource identity check still compares
the exact module and resource.

**Thin C helpers.** Where a library function cannot be bound directly, a
clutch may add a plain C helper to its sources and bind the helper. A helper
wraps one library call, converts only types and constants, and contains no
Slug-aware code. Examples include supplying `SQLITE_TRANSIENT`, converting
`size_t` and `int` lengths, and converting `const unsigned char *` text.
Manifest constants stay deferred.

## Consequences

`slug.db.sqlite` can bind text and blob columns, and its statement module can
take a database handle, without a Slug-aware C adapter. Any clutch with several
modules can share resources between them.

The shim generator emits two C parameters for a length result. The adapter's
cell decoding reads the length for `out str` and `out bytes`. The manifest
model resolves qualified handles across a clutch's modules. Generator, adapter,
and desktop tests cover each.

Still unsupported: `str` and `bytes` return values, structs, buffers,
constants, and handles shared between clutches.

## Migration

None. Existing signatures keep their canonical text, so existing shims stay
current.
