# Typed native calls

## Context

The version 0.13 prototype C ABI exposes the VM to native code through a host
table of roughly forty callbacks. A native function reads each argument and
writes its result through those callbacks, and builds lists and maps one
element at a time. The checks it provides are real, but most are already
derivable from the Slug `foreign` declaration, which carries parameter and
result types, resource types, and arity.

The cost is paid on every call, including the scalar calls that dominate
embedded and game workloads (`gpio.write`, a draw call per sprite). It is paid
again in code size: the loader and its tests are about 3,400 lines, the headers
and helper SDK about 460, and the three first-party clutches (`core`, `fs`,
`sqlite`) are about 500 lines of C, much of it marshalling.

Slug's immutability shapes what a cheaper design may do. Strings, bytes, lists,
and maps are shared freely through reference counts, so a native write through
a pointer into one is visible to every alias and can corrupt a hashed map key.
Fast native access to Slug values must therefore be read-only, and mutation
must go through something that is not a shared value.

[Native extension interface profiles](2026-10-10-native-interface-profiles.md)
defines what a host must offer. This record defines how a native function is
declared and called.

## Decision

**Declarations stay; the manifest binds them.** `foreign` declarations in
Slug source remain the Slug-visible contract and keep their current meaning
under the [declared foreign registry](2026-08-24-declared-foreign-registry.md).
The clutch manifest gains the native binding: the C symbol, its C signature,
and any C layouts. Slug source gains no new syntax and does not mention C
types. Illustrative shape (names are not final):

```toml
[native.types.Color]
fields = ["u8 r", "u8 g", "u8 b", "u8 a"]

[native.constants]
SQLITE_TRANSIENT = -1

[native.functions."slug.gpio".write]
symbol = "gpio_write"
signature = "(i32 pin, bool level) -> void"

[native.functions."slug.db.sqlite".openRaw]
symbol = "sqlite3_open"
signature = "(cstr path, out handle db) -> i32"
```

**Calling convention.** For a call to a declared function the VM validates the
arguments against the Slug declaration (types, arity, resource type, resource
not closed), converts them to the manifest's C types, calls the symbol
through its generated shim entry (see
[Uniform shim entry](2026-10-10-uniform-shim-entry.md)), and converts the
result. The VM itself performs no unsafe call; only the loader does. No host
callback is made during a call.
Native code never receives a VM value, a reference count, or a pointer into
VM storage that it may write through. A conversion or validation failure is a
checked Slug error raised by the VM, never a native fault.

**Type mapping.**

| Manifest type | Slug side | Rule |
|---------------|-----------|------|
| `bool`, `i8`..`i64`, `u8`..`u64`, `f32`, `f64` | `num`, `bool` | A numeric argument must be representable in the C type (whole and in range for integers) or the call fails with a checked error. |
| `str` | `str` | Passed as pointer and length; borrowed, read-only, valid for the call. |
| `cstr` argument | `str` | The VM passes a NUL-terminated temporary copy and rejects interior NUL with a checked error. |
| `cstr` result | `str` or `nil` | The VM copies the bytes before returning and maps a NULL pointer to `nil`. The manifest marks the result borrowed (the default) or owned by a named free function; see [Uniform shim entry](2026-10-10-uniform-shim-entry.md). |
| `bytes` | `bytes` | Passed as pointer and length; borrowed, read-only, valid for the call. |
| `handle` | a declared `resource` | The VM keeps the pointer in a typed resource, checks its type and closed state on every call, and runs the close and destructor registered for it. |
| `buffer` | the `buffer` resource | Pointer and length of the buffer's owned storage. |
| struct name | a Slug `struct` | Copied in and out by the declared layout. |
| `out T` | part of the result | See below. |

**Out parameters.** An `out` parameter is a slot the VM supplies and reads
back. When a signature has out parameters, the Slug-visible result is a list
of the C return value followed by each out value in order, which Slug library
code unpacks. The `foreign` declaration's result annotation describes that
list.

**Errors.** Typed calls need no host error callback, and failures have two
sources:

- **Boundary failures** are detected by the VM or the typed-call adapter, never
  by native code. They use a closed, adapter-owned list of codes: `native.type`
  (an argument of the wrong Slug type), `native.range` (a number not
  representable in the C type), `native.arity`, `native.resource_type`,
  `native.resource_closed`, `native.interior_nul` (a `cstr` argument containing
  NUL), `native.encoding` (invalid UTF-8 in a text result), and load-time
  failures for a missing symbol or a stale shim. A shim that breaks its own
  contract is a `native_contract` error.
- **Library failures** are reported by the native function through its return
  value, an `out` status, a NULL or sentinel result, or a follow-up call such
  as `sqlite3_errmsg`. They cross the boundary as ordinary results. Slug
  library code inspects them and throws an `Error` value whose `type` names the
  failure, for example `Error{type: "native.io", msg: "cannot open file"}`.
  The manifest has no error table.

Usability work (unpacking, error mapping, ordinary Slug-shaped results) lives
in Slug library code over thin, nearly 1:1 foreign declarations.

**Compound values stay out of the native boundary.** SQLite becomes the raw
C API exposed as foreign functions (open, prepare, bind, step, column
accessors, finalize, close) with the current `slug.db.sqlite` and
`slug.db.sqlite.statement` functions rewritten in Slug on top. The Slug
modules' public behavior is unchanged.

**Structs.** A C struct such as `Color` or `Vector2` is represented in Slug by
a `struct` declared in Slug source and bound to a manifest layout. Because the
struct is copied at the boundary, nothing aliases VM storage, and an immutable
`val red = Color{...}` is allocated once and reused. Passing structs by value
to a C function depends on platform calling-convention rules; the first
implementation supports scalar, string, bytes, handle, and buffer parameters
only, and the strategy for by-value structs (a libffi-style dependency or a
thin shim) is decided separately.

**Buffer resources.** `buffer` is a native resource that owns raw mutable
bytes of a size fixed at creation. It is not a Slug value: it cannot be read
as `bytes`, captured as an aggregate, or shared as a value. It is mutated and
filled only through native calls that receive its pointer and length, which
the VM bounds from the resource, and it is released by `close` or its
destructor. Converting it to immutable `bytes` is an explicit operation.
This is the single mutation path for pixels, audio, vertices, SPI and DMA
data, and network buffers, so that immutability of Slug values is never
bypassed.

**Events keep `chan`.** The Slug program creates the channel and passes the
native side a producer capability minted from it, instead of native code
creating the channel. The native library receives a small host function set
at initialization (non-blocking send for nil, bool, integer, float, text, and
bytes; closed-state query; close and release) and no other callbacks. The
semantics of backpressure, revocation, and rejected-value ownership remain
those of the existing producer records.

**Linking.** `[native.libraries]` selects a dynamic library per platform as
today, and the manifest's symbols are resolved with the platform loader. A
static build (firmware, or a game shipped as one executable) reads the same
manifest and links the symbols at build time, generating the registration
glue. The loader and board packages are the only places that may allow
`unsafe`; `slug-vm` and `slug-frontend` keep `unsafe_code = "deny"`.

**Trust.** A clutch and its manifest are trusted code. At load time the host
verifies that each symbol resolves and that the declaration's shape is
compatible with the C signature (for example a `str` parameter cannot bind to
a plain `i32`). For a binding backed by an included header, the generated shim
is compiled against that header, so a wrong parameter count, qualifier, or
integer width is a build error, and a stale shim is rejected by its canonical
signature (see [Uniform shim entry](2026-10-10-uniform-shim-entry.md)).
The host still cannot verify the C types of a binding that has no header or
whose target is a macro, and a wrong signature there is undefined behavior.
Read-only borrowing of `str` and `bytes` is a
contract the C code must honor, not something the host can enforce.

**Replacement plan.** The prototype ABI is frozen at 0.13 and no longer
extended. The replacement proceeds in this order:

1. implement typed calls for scalar, string, bytes, and handle parameters, and
   move the `fs` clutch;
2. move `sqlite` to raw foreign functions plus Slug library code;
3. add the Slug-created-channel producer interface and move `stdin`;
4. remove the prototype loader, its tests, the headers, and the helper SDK.

Before step 4, each stage is measured against the prototype for per-call cost
on a scalar call and for end-to-end behavior on the existing fixtures.

## Consequences

Native calls on the common path become close to a plain C call, and the
first-party clutches shrink to ordinary C or to no C at all (`sqlite` can bind
the library's own API). Embedded and desktop share one declaration format
and one set of validation rules.

Intentionally unsupported: C variadic functions, callbacks from C into Slug,
native threads touching Slug values, by-value structs in the first
implementation, and a mutable view of any Slug value.

Open questions this record does not settle:

- the `native.types` and constants syntax (function, resource, and signature
  keys are specified in [Typed native manifest bindings](2026-10-11-typed-native-manifest-bindings.md));
- whether the default numeric mapping for an unqualified `num` is integer or
  float (the manifest requires an explicit C type, so this affects only
  diagnostics);
- the producer's minting builtin and its revocation rules;
- whether `buffer` conversion to `bytes` copies or transfers ownership;
- how to bind `libsqlite3` (system library or a bundled amalgamation);
- the struct-by-value strategy; and
- a generator that reads a C header or an API description and emits the manifest
  and a `.slug` skeleton.

These records keep their module-level and semantic content, but their C ABI
mechanics are superseded once step 4 lands: [feature-gated C FFI prototype](2026-09-03-feature-gated-c-ffi-prototype.md),
[descriptor validation](2026-09-03-c-ffi-prototype-descriptor-validation.md),
[module lifecycle](2026-09-03-c-ffi-prototype-module-lifecycle.md),
[opaque resources](2026-09-03-c-ffi-prototype-opaque-resources.md),
[SQLite handle](2026-09-04-c-ffi-prototype-sqlite-handle.md),
[SQLite statements](2026-09-04-c-ffi-prototype-sqlite-statements.md),
[cross-platform loader](2026-09-06-cross-platform-c-ffi-prototype-loader.md),
[library-owned lifecycle](2026-09-07-library-owned-ffi-lifecycle.md),
[SQLite compound values](2026-09-07-sqlite-compound-ffi-values.md),
[test-built filesystem clutch](2026-09-07-test-built-filesystem-clutch-ffi.md),
[helper SDK](2026-09-09-ffi-helper-sdk.md), and
[worker library leases](2026-09-15-ffi-worker-library-leases.md). The
[producer capability](2026-09-03-c-ffi-prototype-producer-capability.md),
[backpressure](2026-09-04-c-ffi-prototype-producer-backpressure.md),
[revocation and text ownership](2026-09-04-c-ffi-prototype-producer-revocation-and-text-ownership.md),
and [rejected-value ownership](2026-09-03-native-producer-rejected-value-ownership.md)
records remain the semantic authority for `events`.

Documentation that must be updated: `docs/reference/native-abi.md`,
`docs/reference/experimental-clutches.md`, `docs/reference/ffi-helpers.md`, the
README's clutch paragraph, and the native fixture notes in `AGENTS.md`.

## Migration

Existing Slug programs are unaffected: `slug.io.fs`, `slug.io.stdin`, and the
`slug.db.sqlite` modules keep their public exports and behavior. One
diagnostic difference is accepted. A library failure that a C clutch raises
today as a native fault (for example `native.io` from `slug.io.fs`) is thrown
by Slug library code instead. Error handlers still see the same `type`, but
`msg` no longer starts with the native function's qualified name, and an
uncaught failure is reported as an uncaught throw rather than a native runtime
error. The manifest
key `abi = "slug-ffi-prototype/0.13"` is replaced by `[native.functions]`
bindings, and the C clutch sources under `clutch/*/native/source` are rewritten
to plain exported functions (or removed) as each clutch moves. The
prototype-only Cargo feature and tests are deleted in step 4. No bytecode or
`.cslug` compatibility promise changes.
