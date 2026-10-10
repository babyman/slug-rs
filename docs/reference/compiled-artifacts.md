# Compiled Artifacts

## Status

This document defines the external contract for `.cslug` whole-program
artifacts. Version 1 is implemented by `slug compile source.slug output.cslug`
and `slug run program.cslug`. The latter executes only the decoded closed graph;
it never falls back to source, Clutch, filesystem module, or host module
resolution.

`.cslug` is deliberately a serialization boundary for the Slug VM, not an
independent intermediate language or a stable cross-implementation bytecode
standard. This document specifies the implemented version-1 binary schema.

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

### Embedded firmware builds

An embedded firmware build MAY encode a closed version-1 artifact on its
desktop build host, link its bytes into firmware flash, and decode those bytes
under target-specific finite limits during boot. This is a build-input use of
`.cslug`: a device MUST NOT treat an external file, storage partition, network
payload, or dynamically discovered provider as an artifact source merely
because it supports the embedded path.

The artifact retains foreign declarations and their module-qualified
identities, but it never serializes native functions, native resources, or
native provider state. The firmware target selects and statically links native
providers separately, and it must register implementations matching the
artifact's selected source contracts before execution. A missing, extra, or
duplicate provider is a target-build or host-installation failure, not a
reason to resolve a module dynamically.

The selected entry source, source contracts, provider libraries, and decode
limits are target-build configuration, not fields in the version-1 artifact
schema. They therefore do not alter artifact portability between compatible
VMs, while still allowing a firmware target to apply stricter limits.

## Version-1 encoding eligibility

Version 1 is supported for closed graphs emitted by the Slug frontend. The
unstable public Rust bytecode API does not retain construction provenance, so
the codec validates structural representability rather than asserting that a
`Program` originated in the frontend. Hosts should create artifacts through the
frontend graph API, which supplies the complete resolved import table.

The encoder rejects, with a checked encode error, any value or graph member
outside the version-1 representable set. In particular, it must not serialize
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

## Module and program records

A module-section payload is `module_key:string`, `module_name:string`,
`program_length:uleb128`, and exactly `program_length` bytes. The program
record is an ordered sequence of counted tables. Each count is canonical ULEB128;
each table is encoded in its listed order:

1. bindings and exports as string lists;
2. declarations, callable identities, and optional entrypoint metadata;
3. source-path and source-span tables;
4. global-name, capture-list, schema-field, struct-field, match-pattern,
   interpolation, list-spread, call-argument, selected-call, and select-case
   metadata tables; and
5. chunks, each with its name, arity, parameters, optional callable identity,
   local count, constants, and packed instructions.

Every optional field is one `present:u8` byte (`0` absent, `1` present),
followed by its value only when present. Every boolean is one byte (`0` false,
`1` true); any other byte is invalid. Enum tags are one byte and must name a
defined variant. Lists are a count followed by their elements. A `Program`
table reference is a zero-based ULEB128 index into its corresponding table and
must be in range.

Source spans are `(source_index, line, column)`. Captures are tagged as local
or enclosing-capture slots. Field records contain `name:string` and
`has_default:bool`; parameter records contain `name:string`, `has_default:bool`,
and `variadic:bool`. A declaration retains the source metadata represented by
`ModuleDeclaration`, including binding names, mutable/export/foreign flags,
foreign arity, resource and callable identities, resource signatures,
documentation, and tags. Tags carry their static name only. Their argument
expressions run during module initialization, so each decoded tag begins with
an empty runtime argument list.

Version-1 values are only `nil`, booleans, signed 64-bit integers,
finite IEEE-754 binary64 numbers, UTF-8 strings, and byte strings. Signed
integers use ZigZag-transformed canonical ULEB128. A float is its eight
little-endian IEEE-754 bytes and must be finite. Lists, maps, schemas, structs,
enums, closures, bindings, host callables, channels, tasks, resources, and
overload groups are not valid encoded values; source lowering represents their
construction through bytecode rather than constants.

Packed instructions are `opcode:u8`, `a:uleb128`, `b:uleb128`,
`c:uleb128`, and `span_index_plus_one:uleb128`, where zero means no span. The
opcode byte is the `PackedOpcode` discriminant for VM/bytecode compatibility
version 1; the v1 mapping is fixed by the ordered `PackedOpcode` declaration
in `crates/slug-vm/src/bytecode/chunk.rs`. Its three operands must fit `u32`
and have the meaning assigned by that opcode. The decoder rejects an unknown
opcode before inspecting its operands, then validates every referenced table,
chunk, constant, callable identity, declaration, and instruction target.

Match-pattern, match-type, map-key, call-argument, defer-mode, select-case,
entrypoint-argument, and resource-signature records use their source enum
variant order as one-byte tags, with recursively encoded fields in declaration
order. Recursive match-pattern and match-type depth is limited to 128. An
encoder must emit the canonical pooled form already installed in `Program`;
the decoder must not accept builder-only rich instruction forms.

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

Version 1 does not implement module-level `.cslug` files, encoding of arbitrary
Rust-built bytecode, Clutch integration, zero-copy execution, memory mapping,
generalized IR semantics, or a stable long-lived artifact ABI independent of
the VM. The adopted, unimplemented version-2 module contract is specified in
[Compiled Modules](compiled-modules.md); every other item requires separate
evidence and a later decision.

## Implementation gate for version 1

Version 1 implementation proves round-trip execution, clean incompatible-version
and malformed-input rejection, and direct source execution that does not encode
and decode before reaching the VM.
