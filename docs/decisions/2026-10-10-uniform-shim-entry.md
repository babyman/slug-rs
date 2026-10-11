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
not `void`, followed by each `out` parameter in declaration order. A text or
bytes result is borrowed from native memory and stays valid until the loader
has copied it and then runs `cleanup`. The cleanup record exists so that
result ownership, which this record does not decide, can be adopted without
changing the entry signature.

The return value of the entry is `0` when the results are valid. A nonzero
value reports a shim-detected failure; its mapping to a Slug error is the
separate error-table decision, and until that exists the loader treats it as
a checked native error. Struct-by-value arguments are reserved: a struct will
travel as a pointer to a flat copy in its declared layout, and the shim
assembles the C value.

**One exported table.** A shim library exports a single symbol:

```c
typedef struct slug_shim_binding {
  const char *module;
  const char *name;
  slug_shim_entry entry;
  uint32_t argument_count;
  uint32_t result_count;
  uint64_t signature_hash;
} slug_shim_binding;

typedef struct slug_shim_table {
  uint32_t abi_version;
  uint32_t binding_count;
  const slug_shim_binding *bindings;
  uint64_t table_hash;
} slug_shim_table;

const slug_shim_table *slug_shim_get_table(void);
```

A dynamic loader resolves that one symbol with the platform loader. A static
host (firmware, or a game linked as one executable) calls the same function
directly, so both linking forms use the same table and neither uses per-function
symbol lookup.

**Stale-shim detection.** The manifest and the compiled shim are separate
files. The signature hash is a fixed 64-bit hash of a binding's canonical
manifest signature plus the shim ABI version; the algorithm and canonical
form are specified by the manifest schema. The loader recomputes it from its
manifest and rejects any binding whose hash, argument count, or result count
differs, naming the module and function. A shim built from an older manifest
therefore fails with a checked error and never reaches a call.

**Header checking on every binding.** For each target that can be named as a
function, the generator emits an exact-typed function-pointer constant
initialized from the target symbol, then calls the real symbol:

```c
static int64_t (*const slug_check_add)(int64_t, int64_t) = slug_spike_add;
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
and the table. A check mode regenerates and diffs, so CI can detect a stale
committed shim. The generator does not parse C headers.

**Loader boundary.** The loader's only unsafe call site for typed bindings is
the call through `slug_shim_entry`, plus the one table lookup. The VM and
frontend keep `unsafe_code = "deny"`. The bounded Rust dispatcher and its
shape table stay only as spike evidence and are deleted when the loader lands.

## Consequences

The loader shrinks and stops depending on the set of signatures a clutch uses.
Out parameters, `cstr` handling, result cleanup, `static inline` and macro
targets, and later struct-by-value are all handled inside the shim, where the C
compiler enforces the ABI.

Each call builds a small cell array. At roughly 700 ns per call through the VM
this is not expected to be visible, but it is unmeasured.

Intentionally not decided here: the manifest's final syntax and the canonical
signature form; the native error table; text-result ownership beyond the
reserved cleanup record; struct layout encoding; the buffer resource; the
maximum cell count; per-compiler flag sets and Windows symbol export for
`slug_shim_get_table`; and a Rust-glue alternative for static Rust hosts.

Follow-up spike work that should land before the loader does:

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
