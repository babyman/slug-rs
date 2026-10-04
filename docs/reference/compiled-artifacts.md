# Compiled Artifacts

## Status

This document defines the intended external contract for `.cslug` whole-program
artifacts. The decision to provide that contract is adopted; version 1 has not
yet been implemented, so this repository does not currently create, load, or
accept `.cslug` files.

`.cslug` is deliberately a serialization boundary for the Slug VM, not an
independent intermediate language or a stable cross-implementation bytecode
standard. The version-1 binary schema must be specified here before
implementation begins.

## Artifact identity and scope

A `.cslug` file stores one complete executable Slug program as a closed
compiled module graph. It includes a designated root program, its entry point,
and every resolved compiled dependency needed to execute that graph without a
source resolver. It is not initially a module artifact, compiler cache, memory
snapshot, or serialization of host-language objects.

An artifact is portable across machines running VMs that support its declared
format and VM/bytecode compatibility version. It is not a promise that an
artifact will run on later incompatible VM revisions. A host must reject an
unknown or unsupported version; it must not guess, silently reinterpret, or
partially execute an artifact.

Version 1 will assign fixed magic bytes, byte order, integer encodings, flags,
and canonical encoding rules. Those details are intentionally unassigned until
the executable representation and verifier are designed and reviewed.

## Representation and execution boundary

`CompiledProgram` is the primary executable contract for this implementation:

```text
.slug -> frontend -> CompiledProgram -> VM
```

Normal source execution does not serialize. Saving a program adds the distinct
persistence stage `CompiledProgram -> cslug::encode() -> .cslug`; loading
reverses it through `cslug::decode()` before execution. In-memory compilation
must not encode and immediately decode merely to invoke the VM.

The VM representation drives the schema. Version 1 must encode the
`CompiledProgram` state required by the VM—bytecode, constants, functions,
closed module graph, entry point, source metadata, and executable
invariants—without introducing a VM-independent IR. The exact Rust layouts
remain private and may change; the encoder and decoder evolve with their
supported VM compatibility version.

## Loading and validation

Loading a `.cslug` file is an untrusted-input operation. Before returning a
`CompiledProgram`, the decoder must validate the header, version and flags,
length-delimited sections, resource limits, indices, constants, function and
call metadata, module-graph references, root entry point, and control-flow
targets.

Malformed, truncated, hostile, unsupported, or incompatible artifacts must
produce a checked load error. They must never cause a host panic, unchecked
allocation, out-of-bounds access, or execution of partially validated code.
Exact diagnostic wording is not a compatibility promise unless a future
artifact version says otherwise.

## Compatibility and evolution

`.cslug` compatibility follows VM/bytecode compatibility rather than Slug
source-language versions. A VM that advertises a format/compatibility pair
must accept every valid artifact for that pair within its documented resource
limits.

When the VM's optimal executable representation changes incompatibly, the
artifact version or compatibility version changes with it. The VM rejects older
artifacts cleanly; migration or recompilation is the mechanism for carrying an
artifact forward. No runtime compatibility layer is implied. Compatible
extensions require an explicitly specified mechanism and must preserve the
meaning of existing valid artifacts.

## Initial non-goals

Version 1 does not require module-level `.cslug` files, Clutch integration,
zero-copy execution, memory mapping, generalized IR semantics, or a stable
long-lived artifact ABI independent of the VM. Those require separate evidence
and a later decision.

## Implementation gate for version 1

Before version 1 is implemented, this document must gain the complete binary
schema, compatibility fields, canonical encodings, verifier rules, resource
limits, error mapping, and fixture corpus. The implementation must then prove
round-trip execution, cross-machine compatibility for the same supported VM
pair, clean incompatible-version rejection, malformed-input rejection, and
that source execution still reaches the VM directly.
