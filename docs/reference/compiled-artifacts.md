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

Version 1 begins with this fixed 12-byte header:

| Offset | Size | Field | Version-1 rule |
|---|---:|---|---|
| 0 | 4 | magic | ASCII `CSLG` |
| 4 | 2 | format version | unsigned little-endian integer, `1` |
| 6 | 2 | VM/bytecode compatibility | unsigned little-endian integer, `1` |
| 8 | 4 | flags | unsigned little-endian integer; all bits are zero in version 1 |

Every other fixed-width integer is little-endian. Lengths, counts, and indexes
use unsigned LEB128 and must use their shortest representation. A decoder must
reject an unknown format or compatibility version, any nonzero version-1 flag,
integer overflow, and noncanonical LEB128 input before allocating or executing
from the corresponding value.

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

The decoded graph supplies all runtime imports. An `import()` request whose
logical module identity is absent from that graph, including a dynamically
computed name, must fail as a checked module-load error. Version 1 must not
fall back to a source resolver, Clutch, filesystem, or host module registry.

## Version-1 encoding eligibility

Version 1 accepts only a closed graph emitted by the Slug frontend. It does not
accept arbitrary `slug_vm::Program` values constructed through the unstable
Rust bytecode API. This keeps the artifact boundary defined by source-derived
VM state rather than host object reachability.

The encoder must reject, with a checked encode error, any value or graph member
outside the version-1 source-derived set. In particular, it must not serialize
native functions, declared-native wrappers, native resources, channels, tasks,
closures, bindings, overload groups, or other live host/VM state. It must also
reject a graph that is not closed under its recorded import edges.

## Envelope, graph table, and limits

The header is followed by a sequence of length-delimited sections. Each section
is `kind:u8`, `payload_length:uleb128`, then exactly `payload_length` bytes.
Version 1 permits only these sections, in this order:

| Kind | Name | Count | Payload |
|---:|---|---:|---|
| `1` | graph | exactly one, first | Root-module index, module count, and import-edge table. |
| `2` | module | exactly the graph's module count | One module identity plus its encoded `Program`. |
| `0` | end | exactly one, last | Empty. |

The graph payload contains `root_module_index:uleb128`,
`module_count:uleb128`, `import_count:uleb128`, then one import edge per
entry: `importer_module_index:uleb128`, `logical_name:string`, and
`target_module_index:uleb128`. A string is its byte length as canonical ULEB128
followed by valid UTF-8 bytes. Import edges are the decoded graph's sole
resolver table.

Module sections are sorted by the UTF-8 byte order of their logical module key;
the graph's root and import indices refer to that order. Import edges are sorted
by importer index, logical-name bytes, then target index. Duplicate module keys
and duplicate `(importer, logical name)` edges are invalid. The root and every
edge endpoint must name an existing module.

The default decoder limits are a 64 MiB artifact, 32 MiB section, 4,096
modules, 16,384 import edges, 1 MiB string, 65,535 chunks per module, and
1,048,576 constants, instructions, or entries in any one metadata pool per
module. A decoder must allow an embedding host to choose stricter limits,
including an ESP-specific profile, but it must enforce a finite limit before
allocation or recursive descent.

Version-1 decoding reports one checked load-error category for: truncated
input; bad header; unsupported version or flags; noncanonical or overflowing
integer; invalid UTF-8; limit exhaustion; invalid section structure; invalid
graph/index reference; unsupported source-derived value; and failed VM
executable-invariant validation. No category may return a partially decoded
graph for execution.

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

Version 1 does not require module-level `.cslug` files, encoding of arbitrary
Rust-built bytecode, Clutch integration, zero-copy execution, memory mapping,
generalized IR semantics, or a stable long-lived artifact ABI independent of
the VM. Those require separate evidence and a later decision.

## Implementation gate for version 1

Before version 1 is implemented, this document must gain the complete binary
schema, compatibility fields, canonical encodings, verifier rules, resource
limits, error mapping, and fixture corpus. The implementation must then prove
round-trip execution, cross-machine compatibility for the same supported VM
pair, clean incompatible-version rejection, malformed-input rejection, and
that source execution still reaches the VM directly.
