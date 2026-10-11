# Static selection of typed native providers

## Status

Accepted

## Context

[Typed native calls](2026-10-10-typed-native-calls.md) bind `foreign`
declarations to C symbols through the clutch manifest, and
[Uniform shim entry](2026-10-10-uniform-shim-entry.md) calls every binding
through one generated C entry type found in one exported table. Both records
promise one contract with two linking forms, but specify only the dynamic form
in detail. The static form is a sentence: a static host "calls the same function
directly".

Static hosts today are firmware images and, later, games shipped as one
executable. A [target descriptor](2026-10-09-embedded-static-build-toolchain.md)
selects one provider per module (`module`, `contract`, `library`). The board
package registers Rust `NativeModule` functions, and
`ArtifactHost::install_static_providers` rejects missing, extra, or duplicate
bindings against the artifact's `foreign` declarations. No static host can use
a C library through a manifest.

The dispatch spike and the native-call profiling that followed establish:

- **Dispatch cost does not motivate a separate static path.** A uniform shim
  adds about 0.26 ns to a direct C call. After the 2026-10-11 VM fixes, a
  native call costs about 166 ns over an empty loop and about 190 ns for an
  ordinary Slug function, and none of that is the shim. Static linking is
  chosen for deployment (no dynamic loader, one image), not speed.
- **The value-to-cell adapter needs one `unsafe` call** (the entry, plus the
  cleanup release). It cannot live in `slug-vm`, which keeps
  `unsafe_code = "deny"`, and it must build for `no_std + alloc`.
- **The fixed table symbol collides when linked statically.** Every generated
  shim exports `slug_shim_get_table`; two shims in one image fail to link. The
  spike's generated entry functions also used bare names (`write`,
  `close_handle`) that can collide with C library declarations.
- **Header checking runs where the shim compiles.** For a static image that is
  the firmware or executable build, with the target's C compiler.

## Decision

**One provider per foreign module, of one kind.** A static target selects for
each module that has `foreign` declarations exactly one provider:

- `rust`: a Rust `NativeModule` registered by the host or board package, as
  today; or
- `typed`: a manifest-bound C provider. Its bindings are the clutch manifest's
  native function entries for that module, compiled through a generated shim
  together with the provider's C sources or static library.

A module's functions are not split across providers. The build tool selects;
nothing is chosen at run time, and a static image never falls back to dynamic
loading.

**Descriptor selection.** A provider entry gains a `kind`, which defaults to
`rust` so existing descriptors are unchanged. A `typed` entry names the clutch
whose manifest supplies the bindings instead of a board-package library.
Illustrative shape (names are not final):

```toml
[[providers]]
module = "slug.gpio"
contract = "clutch/slug.embedded.clutch/modules/gpio.slug"
kind = "rust"
library = "slug-esp32-s3::gpio"

[[providers]]
module = "vendor.sensor"
contract = "clutch/vendor.sensor.clutch/modules/sensor.slug"
kind = "typed"
clutch = "clutch/vendor.sensor.clutch"
```

**Build pipeline for a typed provider.** The build tool reads the manifest
through the shared manifest model. It generates the shim translation unit,
compiles it with the target C compiler and the platform's warnings-as-errors
flags, and links it with the provider's objects. It also generates the Rust
registration that wraps each table binding in a `NativeFunction` through the
shared adapter. The build fails before linking when:

- a selected module is missing, extra, or duplicated (the existing check);
- the manifest's bindings for the module do not exactly cover its `foreign`
  declarations;
- a declaration's Slug types are incompatible with the binding's C types;
- the shim fails its header checks; or
- the provider needs a profile the target does not offer.

**Unique table symbols.** Each shim defines its table function under a
provider-unique name, and only a dynamic library exports the
`slug_shim_get_table` alias, as specified in
[Uniform shim entry](2026-10-10-uniform-shim-entry.md). The static
registration calls the provider-unique function, so several typed providers
link into one image.

**One shared adapter.** A single crate, `no_std + alloc` and depending on
`slug-vm`, owns cell encoding, argument and result conversion, the checked
boundary errors, cleanup ordering, and the entry call. It is the only code that
calls a shim entry, for both the desktop loader and static hosts. It permits
`unsafe` only for the entry and release calls. `slug-vm` and `slug-frontend`
keep `unsafe_code = "deny"`.

**The same stale-shim check.** A static build generates its shim and its
registration from one manifest, so a mismatch is normally impossible. The
adapter still compares each binding's signature against the manifest when the
provider is installed, exactly as the dynamic loader does. This costs one
comparison per binding at startup and catches a stale prebuilt static library.

**Profiles are checked at build time.** A target descriptor lists the profiles
it offers (`core` by default for firmware). A provider's manifest states the
profile it requires, and the build rejects a provider that needs more.

## Consequences

Firmware and single-executable hosts can use C libraries, such as vendor HAL or
sensor drivers, under the same manifest, header checks, and checked boundary as
desktop clutches. One manifest drives the desktop dynamic library and the
static image. Rust providers are unchanged and can sit next to typed providers
in one image.

A target that selects a typed provider needs a C compiler for that target. The
ESP32-S3 toolchain already provides one.

Intentionally unsupported:

- splitting one module's functions across providers;
- choosing a provider at run time, or dynamic loading on a static target;
- typed bindings implemented in Rust without C (a generator for typed Rust glue
  is a separate decision); and
- the `events` profile on targets without a polled host loop.

Tests that must exist before the first typed static provider ships:

- a desktop test that statically links two generated shims into one binary;
- build rejection for missing coverage, type incompatibility, header mismatch,
  and an unsupported profile; and
- a cross-build gate for a typed fixture provider on the ESP32-S3 target.

[Build embedded firmware from closed `.cslug` artifacts](2026-10-09-embedded-static-build-toolchain.md)
and [Uniform shim entry](2026-10-10-uniform-shim-entry.md) link to this record.
The ESP32-S3 README documents the descriptor's `kind` field when it is
implemented.

## Migration

None for existing builds: descriptors without `kind` select Rust providers, and
the target-descriptor format version is unchanged. Manifest keys and the type
compatibility check are specified in
[Typed native manifest bindings](2026-10-11-typed-native-manifest-bindings.md).
Signatures, result ownership, and boundary errors follow
[Uniform shim entry](2026-10-10-uniform-shim-entry.md) and
[Typed native calls](2026-10-10-typed-native-calls.md). This record does not
change source syntax, bytecode, or `.cslug`.
