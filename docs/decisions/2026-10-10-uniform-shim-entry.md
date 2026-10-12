# Uniform entry point for generated shims

## Context

[Generated shims for typed native calls](2026-10-10-generated-shims-for-typed-native-calls.md)
selects header-checked C shims over a bounded Rust dispatcher. The spike that
supports it has a limit worth recording. Its generated shims keep each target's
own signature (`slug_shim_add(int64_t, int64_t)`), and the loader reaches them
through the same finite shape table and typed function-pointer casts it uses
for direct calls. The two candidates therefore differ only by one trivial
wrapper call, which explains why the shim showed no measurable cost. It also
means that, as spiked, the loader still carries the shape-specific `unsafe`
code, an arity limit, and a closed set of supported signatures, which the
shim decision says should not be a production path.

The spike also proved header checking only through a hand-written negative case (a deliberately incompatible
function-pointer declaration that fails under
`-Werror`). A generator needs that check on every binding, and needs it to
cover integer width, which implicit C conversions would otherwise hide.

This record specifies a shim shape that removes the loader-side shape table.
It narrows the generated-shims record; it does not change the source-level or
lifecycle decisions in [Typed native calls](2026-10-10-typed-native-calls.md).

## Decision

**One entry type.** Every generated shim has the same C signature. Arguments
and results travel as arrays of fixed-size cells:

```c
typedef struct slug_value {
  union {
    int64_t i64;
    uint64_t u64;
    double f64;
    const void *pointer;
  } as;
  uint64_t length; /* str, bytes, and buffer length; otherwise 0 */
} slug_value;

typedef struct slug_shim_cleanup {
  void (*release)(void *context);
  void *context; /* release is NULL when there is nothing to release */
} slug_shim_cleanup;

typedef int32_t (*slug_shim_entry)(const slug_value *arguments,
                                   slug_value *results,
                                   slug_shim_cleanup *cleanup);
```

The loader needs exactly one Rust function-pointer type for every binding. It
no longer has a shape table, an arity limit beyond a documented maximum cell
count, or per-signature casts.

**Cell encoding.** The loader fills one argument cell per Slug-visible
parameter, in declaration order, by the manifest's C type:

| Manifest type   | Cell                                                                                   |
|-----------------|----------------------------------------------------------------------------------------|
| `bool`          | `u64`, 0 or 1                                                                          |
| `i8`..`i64`     | `i64`, range-checked by the loader before the call                                     |
| `u8`..`u64`     | `u64`, range-checked by the loader before the call                                     |
| `f32`, `f64`    | `f64`                                                                                  |
| `str`, `bytes`  | `pointer` and `length`, borrowed and read-only for the call                            |
| `cstr` argument | `pointer` to a loader-made NUL-terminated copy; interior NUL is rejected by the loader |
| `handle`        | `pointer`, after the loader checks its type and closed state                           |
| `buffer`        | `pointer` and `length` of the buffer's owned storage                                   |

Results use the same encoding. `results[0]` is the C return value when it is
not `void`, followed by each `out` parameter in declaration order. A `cstr`
result's length is set by the shim with `strlen`, and a NULL text result
becomes `nil`.

**Result ownership.** The loader always copies a text or bytes result before
the call returns to Slug; invalid UTF-8 in a text result is a checked error,
never a lossy conversion. The manifest marks each text or bytes result
(return value or `out` parameter) as either:

- **borrowed** (the default): the library keeps ownership, for example
  `sqlite3_column_text` or a per-handle line buffer. The loader copies it and
  frees nothing; or
- **owned by a named free function**, for example `cstr owned(sqlite3_free)`
  or `owned(free)`: the caller must free it.

For an owned result the generator emits a release function and points the
cleanup record at the call's results array, so one record frees several owned
results without an allocation:

```c
static void slug_shim_read_text_release(void *context) {
  slug_value *results = context;
  sqlite3_free((void *)results[0].as.pointer);
}
```

The free function gets the same exact-type header check as the target. The
loader keeps the results array alive and unmodified until release has run,
and runs release whenever the shim set it, including when the entry returns a
failure. A shim sets `release` only after it has filled its results. Handing
native memory to Slug without a copy is not supported; that belongs to the
`buffer` resource.

**Entry status.** The entry returns `0` when its results are valid. A nonzero
value means the shim broke its own contract, and the loader raises a
`native_contract` error naming the binding and the status. A library's own
failure is never reported this way: the shim returns the library's status,
NULL, or sentinel as an ordinary result, and Slug library code maps it to an
error (see [Typed native calls](2026-10-10-typed-native-calls.md)).
Struct-by-value arguments are reserved: a struct will travel as a pointer to a
flat copy in its declared layout, and the shim assembles the C value.

**One table per shim.** Each shim defines one binding table:

```c
typedef struct slug_shim_binding {
  const char *module;
  const char *name;
  const char *signature; /* canonical signature text */
  slug_shim_entry entry;
  uint32_t argument_count;
  uint32_t result_count;
} slug_shim_binding;

typedef struct slug_shim_table {
  uint32_t abi_version;
  uint32_t binding_count;
  const slug_shim_binding *bindings;
} slug_shim_table;

const slug_shim_table *slug_shim_get_table(void);
```

A dynamic loader resolves that one symbol with the platform loader. A static
host (firmware, or a game linked as one executable) may link several shims
into one image, where a shared symbol name would fail to link. Each generated
shim therefore defines its table function under a provider-unique name, such
as `slug_shim_table_vendor_sensor`, and a shim built as a dynamic library also
exports `slug_shim_get_table` as an alias for it. The static host calls the
provider-unique function directly. Both linking forms use the same table, and
neither uses per-function symbol lookup. How a static target selects and
registers shims is specified in
[Static selection of typed native providers](2026-10-11-static-typed-provider-selection.md).

**Stale-shim detection.** The manifest and the compiled shim are separate
files. Each binding carries its canonical signature: text that both the
generator and the loader derive from the parsed manifest, never from its
formatting. It contains everything that changes the generated C for that
binding: the shim ABI version, module, function, C symbol, each parameter's C
type and direction, the result type, the ownership of each text or bytes
result, and the C type names of pointers and handles. Parameter names,
comments, and manifest layout are excluded. The manifest model specifies the
exact spelling, which must be deterministic and platform-independent.

The loader rebuilds each binding's signature from its manifest and rejects
any binding whose signature, argument count, or result count differs. The
diagnostic names the module and function and shows both signatures. A shim
built from an older manifest therefore fails with a checked error and never
reaches a call. Comparing text rather than a hash costs a few dozen bytes per
binding, avoids specifying a hash algorithm, and cannot collide.

**Header checking on every binding.** For each target that can be named as a
function, the generator emits an exact-typed function-pointer constant
initialized from the target symbol, then calls the real symbol:

```c
static int64_t (*const slug_shim_check_add)(int64_t, int64_t) = slug_spike_add;
```

This generalizes the spike's negative case. The types come from the manifest
and the compiler compares them with the header's declaration, so a wrong
parameter count, pointer-versus-integer mismatch, qualifier mismatch, or
integer width mismatch (`int` versus `int64_t`) is a diagnostic. Targets that
are macros or cannot be addressed use a direct call with no pointer check; the
manifest must mark them as such, and they rely on `-Wconversion
-Wsign-conversion` instead. Shims build with warnings as errors under the
platform compiler's equivalent flags, so these diagnostics fail the build.

**Generator.** The generator is Rust code over the shared manifest model, not a
shell script, so one parser serves the desktop CLI, clutch builds, and a
firmware `build.rs`. The model lives in a crate that depends on neither
`slug-vm` nor `slug-frontend`. Inputs are the manifest's bindings (symbols,
C signatures, layouts, constants, and the headers to include); output is one
deterministic C translation unit containing the entry functions, the checks,
and the table. Every generated function except the table function has internal
linkage and a `slug_shim_` prefix, so it cannot collide with a C library
declaration such as `write`. A check mode regenerates and diffs, so CI can
detect a stale committed shim. The generator does not parse C headers.

**Loader boundary.** The loader's only unsafe call sites for typed bindings are
the call through `slug_shim_entry` and the cleanup `release` call, plus the one
table lookup. The VM and
frontend keep `unsafe_code = "deny"`. The bounded Rust dispatcher and its
shape table stay only as spike evidence and are deleted when the loader lands.

## Consequences

The loader shrinks and stops depending on the set of signatures a clutch uses.
Out parameters, `cstr` handling, result cleanup, `static inline` and macro
targets, and later struct-by-value are all handled inside the shim, where the C
compiler enforces the ABI.

Each call builds a small cell array. The spike measured the uniform entry
within noise of the other dispatch candidates; a plain C call through the shim
costs about 0.26 ns more than a direct call.

The manifest keys, the exact canonical signature spelling, the maximum cell
count, compiler flags, and Windows export are specified in
[Typed native manifest bindings](2026-10-11-typed-native-manifest-bindings.md). Intentionally not decided here: struct layout encoding; the buffer
resource; and a Rust-glue alternative for static Rust hosts.

Follow-up spike work completed before the loader (see the
[spike results](../planning/completed/typed-native-call-dispatch-spike-results.md)):

- rewrite the spike's generated shims to the uniform entry and call them
  through it, which tests this decision directly;
- time a direct C call against a shim call in a plain C loop, to measure the
  shim itself;
- add the two controls for the VM call path (a Slug loop calling a Slug
  function, and one calling a statically registered Rust native), so the
  measured nanoseconds can be attributed; and
- prove each of the width, qualifier, and arity mismatches fails to compile
  on macOS, Linux, and Windows.

## Migration

None. The spike fixture is private, and existing Slug programs and the 0.13
prototype ABI remain unchanged.
