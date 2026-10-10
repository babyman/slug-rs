# Build embedded firmware from closed `.cslug` artifacts

## Status

Accepted

## Supersedes

- [Keep `.cslug` off the ESP32-S3 deployment path](2026-10-04-cslug-esp-deployment-superseded.md)
- [General private frozen firmware programs](2026-10-08-general-frozen-firmware-programs-superseded.md)

## Context

The ESP32-S3 proof now compiles a Slug entry program and selected embedded
source contracts on the desktop build host, encodes their closed graph as a
version-1 `.cslug` artifact, embeds that artifact in firmware flash, and runs
it through the shared `no_std + alloc` VM. It registers GPIO, time, and builtin
functions as statically linked native providers.

That proves the execution boundary, but the implementation still selects one
`blink.slug` entrypoint, three module contracts, their providers, artifact
limits, and the output filename directly in the firmware package. Adding an
application or board would require editing firmware implementation code. There
is no build-level declaration that proves every foreign declaration in the
artifact has a matching statically linked provider.

## Decision

Embedded deployment uses the following build boundary:

```text
.slug entry + target descriptor
  -> desktop frontend resolves a closed graph using only selected contracts
  -> `.cslug` artifact validated against the target limits
  -> firmware image statically links provider libraries and artifact bytes
  -> device decodes the flash-resident artifact and registers those providers
  -> board flasher writes the firmware image
```

A versioned target descriptor is the authority for one firmware build. It
names the entry source, board/runtime package, artifact limits, and every
source contract plus statically linked provider required by the target. It is
not a Slug module resolver and it does not grant runtime discovery: the build
tool constructs a `StaticModuleResolver` only from the descriptor's selected
contracts.

The build must fail before firmware linking when the entry graph imports an
unselected module, a selected source contract has an unimplemented `foreign`
declaration, a provider registers an undeclared foreign function, or the
encoded graph exceeds the descriptor's limits. A provider is linked as normal
target-native code (initially Rust crates; a later C provider may be linked by
the board package) and registered by its `(module, function)` identity. Native
code, native state, and hardware handles are never encoded in `.cslug`.

The initial command surface is an implementation task, but it must take the
entry source and target descriptor explicitly rather than encode a board or
program name in a build script. Cargo may invoke that tool from a board
package. The generated artifact is a firmware build input, not a runtime file:
the device accepts only the bytes linked into its firmware image.

This supersedes the ESP32-S3 deployment decision in [Keep `.cslug` off the
ESP32-S3 deployment path](2026-10-04-cslug-esp-deployment-superseded.md), the private
frozen-image representation chosen in [General private frozen firmware
programs](2026-10-08-general-frozen-firmware-programs-superseded.md), and the
firmware-specific portions of [Static Clutch
providers](2026-10-04-static-clutch-providers.md). Those records remain useful
history; this record is the authority for future embedded builds.

## Consequences

The Slug source language and grammar do not change. `import(...)` and
`foreign` retain their existing contracts; target selection happens outside
source and is explicit in the firmware build.

The generic compiler, descriptor validation, and provider-contract validation
must live outside a specific board package. Board packages own only startup,
memory/linker policy, provider-library selection, static registration, and
flashing instructions. The ESP32-S3 PinPulse package becomes the first target
descriptor consumer rather than the template for every application.

Each target must have desktop tests for graph closure, provider parity,
rejection of unselected imports and missing providers, and limit exhaustion.
It also needs a cross-build gate; hardware flashing remains a separate board
acceptance step. `.cslug` remains a versioned, validated program artifact, but
this decision does not authorize device storage, network loading, dynamic
libraries, or runtime provider discovery.

## Migration

Replace the ESP32-S3 build script's hard-coded entrypoint, contracts, and
artifact filename with an explicit descriptor-driven invocation. Preserve
`programs/blink.slug` as the descriptor's documented default proof application
until at least one independently selected application has passed the same
desktop and cross-build validation.
