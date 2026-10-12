# Typed native manifest bindings

## Context

[Typed native calls](2026-10-10-typed-native-calls.md),
[Uniform shim entry](2026-10-10-uniform-shim-entry.md), and
[Static selection of typed native providers](2026-10-11-static-typed-provider-selection.md)
fix the binding model but leave the details an implementation needs open:

- the manifest keys;
- the exact canonical signature spelling;
- how a handle is closed;
- where Slug and C types are compared;
- the maximum cell count;
- compiler flags and Windows export; and
- how shims are built.

[Step 1 of the typed native-call plan](../planning/completed/typed-native-calls-step-1.md)
needs all of them before its manifest model and generator can be written.

Two facts constrain the answers:

- **Close behaviors differ.** The spike named a handle's C close function in
  Rust code. Among the clutches that will move, `slug.io.fs.close` returns
  `nil` and is idempotent, while `slug.db.sqlite.close` returns a status.
- **The VM keeps only resource identities.** At run time it records the
  resource identity of each `foreign` parameter, not its Slug type.

## Decision

**Manifest keys.** A clutch's `[native]` table uses either the prototype
form, with `abi = "slug-ffi-prototype/0.13"`, or the typed form below. Having
both is a manifest error. The `[modules]` table and `[native.libraries]` keep
their current meaning. Illustrative `slug.io.fs` shape:

```toml
[native]
profile = "core"
headers = ["native/source/slug_io_fs.h"]
sources = ["native/source/fs.c"]
link = []

[native.libraries]
"macos-aarch64" = "native/macos-aarch64/libslug_io_fs.dylib"

[native.resources."slug.io.fs".File]
type = "slug_fs_file"
release = { symbol = "slug_fs_close", signature = "(handle<File> file) -> void" }

[native.functions."slug.io.fs".openReadRaw]
symbol = "slug_fs_open_read"
signature = "(cstr path) -> handle<File>"

[native.functions."slug.io.fs".readLineRaw]
symbol = "slug_fs_read_line"
signature = "(handle<File> file, out cstr line) -> i32"

[native.functions."slug.io.fs".closeRaw]
symbol = "slug_fs_close"
signature = "(close handle<File> file) -> void"
```

The `[native]` table:

- `profile` names the profile the provider needs, `core` or `events`.
- `headers` are included by the generated shim, and they must declare every
  target symbol.
- `sources` are compiled with the shim, both into a dynamic library and into
  a static image.
- `link` names extra system libraries.

A function entry has a `symbol`, a `signature`, and an optional
`macro = true` for a target that cannot be addressed and therefore gets no
exact-type check.

**Signatures.** A signature is a parameter list and a result:
`(param, ...) -> result`. A parameter is an optional direction, a type, and an
optional name.

The types supported in step 1:

- `bool`;
- `i8`, `i16`, `i32`, `i64`;
- `u8`, `u16`, `u32`, `u64`;
- `f32`, `f64`;
- `str` and `bytes`, as arguments only;
- `cstr`; and
- `handle<R>`, where `R` names a resource declared under
  `[native.resources]` for the same module.

A result is `void` or one of those types, except `str` and `bytes`. A `cstr`
in result position, as a return or an `out` value, may add `owned(free_fn)`,
for example `out cstr owned(free) line`. Without it, the result is borrowed.

There are two directions:

- **`out`** makes the parameter a result slot.
- **`close`** marks the one handle parameter that the call consumes.

A NULL `handle` or `cstr` result becomes `nil`.

**C types.** The generated shim checks each target against these C types:

| Manifest | C parameter or result |
|---|---|
| `bool` | `bool` |
| `i8`..`i64`, `u8`..`u64` | `int8_t`..`int64_t`, `uint8_t`..`uint64_t` |
| `f32`, `f64` | `float`, `double` |
| `str` | `const char *`, `size_t` (two C parameters) |
| `bytes` | `const uint8_t *`, `size_t` (two C parameters) |
| `cstr`, borrowed | `const char *` |
| `cstr owned(free_fn)` | `char *` |
| `handle<R>` | `ctype *` |
| `out T` | a pointer to the type of `T` |

A free function named by `owned` has the type `void (void *)`, like `free`
and `sqlite3_free`.

**Canonical signature text.** The generator and the loader both build this
text from the parsed model:

```text
slug-shim/<abi>;<module>;<function>;<symbol>;(<p>,<p>,...)-><result>[;macro]
```

Its rules:

- A parameter is spelled `[direction ]type`, with directions `out` and
  `close`.
- A handle is spelled `handle<R:ctype>`, which carries the C type from the
  resource entry.
- Ownership is spelled ` owned(free_fn)` after the type.
- Names, comments, whitespace, and key order are not part of the text. There
  is no other whitespace, and every token is ASCII.

For example:

```text
slug-shim/1;slug.io.fs;readLineRaw;slug_fs_read_line;(handle<File:slug_fs_file>,out cstr)->i32
```

**Resource close and release.** A resource entry gives the handle's C type
(the shim passes `type *`) and a `release` binding. The adapter calls
`release` at most once: when the VM closes a still-open resource at teardown,
or from the resource destructor. The release result is ignored.

A function whose handle parameter is marked `close` is an explicit close:

- If the resource is already closed, the adapter makes no native call and
  every result is `nil`, so explicit close is idempotent.
- Otherwise the adapter marks the resource closed and calls the function.
  Release is then never called for that resource, and the function's results
  return to Slug unchanged, for example a status for SQLite.
- A close function must leave the handle unusable whatever it returns.

Any other use of a closed resource fails with `native.resource_closed`.

Release and close bindings are separate table entries. A release binding's
table name is `release:<R>`, which cannot collide with a Slug identifier.

**Type compatibility.** The host that compiles a clutch's module source
compares each `foreign` declaration with its manifest binding before the
module is installed:

- the desktop loader, at activation; or
- the static build tool, before linking.

A mismatch is a checked module error on the desktop and a build failure for a
static target. The runtime adapter checks only arity, resource identity, and
closed state. Every parameter and the result of a bound `foreign` declaration
must be annotated, and:

| Manifest | Slug annotation |
|---|---|
| `bool` | `bool` |
| integer and float types | `num` |
| `str`, `cstr` argument | `str` |
| `bytes` | `bytes` |
| `handle<R>` argument | `R` |
| `void` result | `nil` |
| `cstr` result | `str` or `str\|nil` |
| `handle<R>` result | `R\|nil` |
| any `out` parameter | `list` (the C result followed by each `out` value) |

A `cstr` result annotated `str` without `nil` fails with `native.type` if the
call returns NULL.

At run time an integer parameter accepts only a Slug integer, and a float
parameter accepts an integer or a float. A value outside the C type's range
fails with `native.range`, as does an unsigned result above the largest Slug
integer. The number of non-`out` parameters must equal the
declaration's arity.

**Cell limit.** A binding has at most 16 argument cells and 16 result cells.
The generator and the loader both reject a larger binding, so the adapter
uses fixed-size arrays and makes no allocation per call.

**Compiler flags and export.**

- clang and gcc build shims with
  `-std=c11 -Wall -Wextra -Wconversion -Wsign-conversion -Werror`.
- MSVC is unsupported until it is needed. Windows builds use clang, as the
  spike's CI already does.
- The generator emits a `SLUG_SHIM_EXPORT` macro. It expands to
  `__declspec(dllexport)` for a Windows dynamic build and to default
  visibility elsewhere, and it is applied only to the `slug_shim_get_table`
  alias.
- The table function is `slug_shim_table_<provider>`, where the build names
  the provider. The `slug_shim_get_table` alias is compiled only when
  `SLUG_SHIM_DYNAMIC` is defined, so one generated file serves both linking
  forms.

**Building shims.** Shims are generated when a clutch is staged or a static
image is built, and first-party clutches never commit generated C. The
generator is a library in the manifest crate:

- a small binary over it serves clutch staging;
- static builds call it from their build script; and
- its check mode serves clutch authors who choose to commit a generated shim.

## Consequences

The manifest model, generator, and loader share one parsed form, so a shim and
its loader cannot disagree about a signature without the check reporting it.
Close behavior lives in the manifest, so neither the adapter nor a clutch
needs per-resource Rust code. Moving `slug.io.fs` and `slug.db.sqlite` keeps
their idempotent and status-returning closes.

Type compatibility is a compile-time property. Clutch modules must annotate
every bound `foreign` declaration; the `slug.io.fs` contract already does.
Variadic declarations, such as SQLite's `exec` and `query`, cannot bind to a
C function directly. They become Slug code over raw foreign functions, as
[Typed native calls](2026-10-10-typed-native-calls.md) already plans for
SQLite.

Intentionally unsupported in this record:

- `str` and `bytes` results, which need a length convention;
- structs, `buffer`, constants, and `events` producer bindings;
- MSVC; and
- more than 16 cells.

Each needs its own addition when a clutch requires it.
[Typed native length results and shared handles](2026-10-11-typed-native-length-results-and-shared-handles.md)
adds length-delimited `out str` and `out bytes` results and handles shared
between a clutch's modules, and
[Typed native producers](2026-10-11-typed-native-producers.md) adds the
`producer` argument for the `events` profile.

The step 1 plan's phase 1 implements this record. The typed native calls,
uniform shim entry, and static provider records link here for the items they
left open.

## Migration

None. The prototype manifest form is unchanged. A clutch adopts the typed form
when it moves off the prototype ABI.
