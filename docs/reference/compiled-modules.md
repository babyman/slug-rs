# Compiled Modules

## Status

This is the adopted, unimplemented contract for version-2 `.cslug` artifacts.
Version-1 files remain closed whole-program artifacts as specified in
[Compiled Artifacts](compiled-artifacts.md).

## Artifact roles

Version 2 retains `CSLG` magic, little-endian fixed fields, and canonical
ULEB128. Its header has format version `2`, a VM/bytecode compatibility
version, and a flags word whose bit zero is the artifact role (`0` program,
`1` module); every other bit is reserved and zero. Version-1 readers reject
version 2.

| Role | Contents | Direct execution | Import provider |
|---|---|---:|---:|
| `program` | Closed graph and root entry point | Yes | No |
| `module` | One module, public interface, and requirements | No | Yes |

Program artifacts resolve imports only from their decoded graph. They never
fall back to source, Clutches, files, or host module lookup at execution time.
Module artifacts are build inputs, not executable package files.

## Provider resolution and linking

A module artifact declares exactly one valid dotted `provided_name`. A project
or Clutch manifest explicitly selects a source or module-artifact provider for
each logical import name. A duplicate provider without an explicit selection,
an absent provider, or a provider whose declared name differs from the request
is a checked build error. There is no implicit `.slug` versus `.cslug`
precedence rule.

Each module-artifact static import records its requested logical name and the
SHA-256 digest of the canonical public interface expected by the importer. The linker resolves
every requirement, validates the selected provider's interface, and gathers
source-compiled and module-decoded programs into one closed `program` graph.
It writes final import edges using final graph indices. At execution, the VM
cannot distinguish whether a member began as source or a compiled module.

Dynamic import names remain valid language behavior but the final executable
artifact may satisfy only names present in its graph; they never trigger a
runtime provider search.

## Public compiler interface

The module interface is canonical data, not a Rust snapshot or VM layout. It
must retain all information a source importer needs without reparsing source:

- exported value names, mutability, and canonical static types;
- callable overloads, generic parameters, parameter/default/variadic shape,
  result type, and stable callable identities;
- exported schemas, structs, enums, aliases, and resources with nominal
  module/name identities and member metadata;
- foreign callable/resource metadata required for checked binding; and
- retained public documentation/tags plus a SHA-256 digest over the canonical
  section.

It excludes initialized values, closures, native pointers, resources, tasks,
channels, allocator state, and host configuration. The frontend owns conversion
between its semantic import snapshot and this section. The decoder validates an
interface before the frontend uses it.

## Native capabilities and Clutches

Artifacts encode requirements, never native code or library paths. A native
requirement identifies its logical module, binding, callable/resource identity,
and provider ABI or capability identifier. The target host verifies every
requirement before execution.

Desktop Clutches may provide source or compiled modules and dynamically load a
selected native provider. Firmware profiles select the same logical provider
but link its implementation statically. Version-0 Clutch manifests remain
source-only; a future manifest must add explicit artifact and target-native
fields rather than reinterpret the existing `source` field.

## Firmware use

An ESP build runs the ordinary linker on the desktop host, embeds the final
closed program artifact as immutable flash bytes, and decodes it under finite
ESP-specific limits. It registers only statically linked providers selected by
the firmware profile. The device resolves neither source nor module artifacts,
Clutches, or dynamic libraries.

Format/VM mismatch, malformed artifact/interface, missing provider, interface
mismatch, or missing native capability are checked diagnostics. Recompilation
is the recovery path; execution never recompiles or silently replaces a
provider.
