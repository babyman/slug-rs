# Frozen firmware image boundary

## Context

The ESP32-S3 proof currently loads a checked, firmware-local program image
into Slug's private in-process bytecode representation. This avoids source
parsing, filesystem access, and a desktop loader on the device. It does not,
however, establish how authored Slug source becomes that image.

MicroPython demonstrates the value of cross-compiling selected application
modules on a development machine and freezing the result into firmware flash.
Slug must adopt that deployment shape without treating private VM bytecode as
a portable `.cslug` file. The compiled-artifact reference requires a complete
versioned schema and verifier before any `.cslug` version is published.

## Decision

Slug may add a **private frozen firmware image** path before `.cslug` version
1 exists. A desktop build tool compiles selected Slug entry modules and emits
Rust build input that is linked into one firmware image. The ESP host loads the
generated image only from firmware flash.

The frozen image is not a file format, package format, cache, or interchange
artifact. It is coupled to the exact firmware build and private VM revision;
the firmware loader must not accept it from device storage, a network, or an
external file.

The generator and loader must retain these boundaries:

- source compilation, dependency resolution, and capability declaration occur
  on the desktop build host;
- the device validates generated image bounds and references before execution;
- the firmware host explicitly registers device capabilities such as UART
  `println`;
- generated image changes remain an implementation detail and do not publish
  opcode numbers, Rust layouts, or a compatibility promise.

`.cslug` remains unavailable until its separate implementation gate in
[`compiled-artifacts.md`](../reference/compiled-artifacts.md) is satisfied.

## Consequences

### Positive

Firmware can execute application code from flash without a parser, filesystem,
or an accidental portable-bytecode promise. The desktop compiler remains the
single authority for Slug source semantics, and the device retains the shared
VM plus checked bytecode validation.

### Negative

Frozen programs require a firmware rebuild and reflash. The first generator
must deliberately support only the metadata and operations needed by its
selected entry module; it cannot claim that arbitrary Slug modules are
portable artifacts.

### Neutral

This supersedes the source-at-boot portion of
[Embedded host capabilities](2026-10-03-embedded-host-capabilities.md) for the
ESP32-S3 deployment path. It does not change the language, desktop loader, or
portable `.cslug` contract.

## Migration

None. The existing firmware-local image remains a proof fixture until the
desktop generator replaces it.
