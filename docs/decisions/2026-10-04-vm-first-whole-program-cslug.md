# Make `.cslug` VM-first whole-program artifacts

## Status

Accepted

## Supersedes

[Adopt portable compiled modules](2026-08-16-portable-compiled-modules-superseded.md).

## Context

The earlier [portable compiled modules](2026-08-16-portable-compiled-modules-superseded.md)
decision defined `.cslug` as a VM-independent, stable compiled-module
contract. The ESP path instead needs a small deployable executable unit aligned
with the VM and the existing private frozen-firmware-image work. No `.cslug`
version has been published or implemented, so this design can change without
migrating released artifacts.

## Decision

`.cslug` is the versioned, validated serialized form of a VM
`CompiledProgram`, initially for complete executable programs rather than
individual modules. The VM's executable representation determines the format;
Slug will not add a VM-independent IR for this purpose.

Normal source execution remains `.slug -> frontend -> CompiledProgram -> VM`.
Encoding is a persistence or transport boundary, and decoding must fully
validate untrusted bytes before returning a program to the VM. Artifacts are
portable only between machines whose VMs support the artifact's declared
format and VM/bytecode compatibility version.

Incompatible artifacts are rejected with checked load errors. Migration or
recompilation, not a runtime compatibility layer, carries programs forward.
The existing private frozen firmware image remains distinct: it is coupled to a
firmware build and is not `.cslug`.

This supersedes [Adopt portable compiled modules]
(2026-08-16-portable-compiled-modules-superseded.md).

## Consequences

- `CompiledProgram` remains the primary in-memory contract; source execution
  never incurs a needless encode/decode round trip.
- The decoder is a VM-adjacent untrusted-input boundary and must preserve the
  checked-error guarantee for malformed or hostile files.
- Initial `.cslug` files include the entry point and complete compiled module
  state. Clutches and module loaders continue to use source until a separate
  decision establishes a module-artifact need.
- Private VM refactors may require a new compatibility version and cleanly
  reject old artifacts rather than constrain runtime design.

## Migration

None. No `.cslug` artifact version has been emitted or accepted. Existing
private frozen firmware images remain firmware-build input and are unaffected.
