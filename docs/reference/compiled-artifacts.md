# Compiled Artifacts

## Status

This document defines the intended external contract for portable Slug compiled
modules.  The decision to provide that contract is adopted; version 1 has not
yet been implemented, so this repository does not currently create, load, or
accept `.cslug` files.  The version-1 binary encoding and executable instruction
set must be added here before implementation begins.

The contract is normative once an artifact version is published.  It is
separate from the source-language grammar and from any implementation's private
bytecode representation.

## Artifact identity

A compiled module is stored in a file with the `.cslug` extension.  It is a
portable executable module, not a compiler cache, memory snapshot, or a
serialization of host-language objects.

An artifact version identifies its binary schema.  A language compatibility
version identifies the Slug source and runtime rules assumed by the module.
Both versions are mandatory and independently checked when loading.  A loader
must reject an artifact when it does not support either version; it must not
guess, silently reinterpret, or execute an unknown version.

Version 1 will assign a fixed magic value, byte order, integer encodings, and
canonical encoding rules.  Those details are intentionally unassigned until
the executable representation is designed and reviewed.  They must be
specified before any version-1 artifact is written.

## Required module contents

Every published artifact version must encode, or provide a lossless equivalent
for, the following information:

- artifact and language compatibility versions;
- module identity and declared imports/exports;
- constants, functions, and executable code;
- enough source identity and span information to produce source-oriented
  diagnostics when available;
- dependency identities and the compatibility requirements used to resolve
  them;
- all information needed to validate references before executing code.

The artifact format must not require a Rust type layout, pointer width,
endianness of the producing host, Go objects, or a particular VM instruction
layout.  Implementations may use different internal execution strategies after
loading the same artifact.

## Loading and validation

Loading a `.cslug` file is an untrusted-input operation.  Before execution, a
loader must validate its header, version requirements, length-delimited
sections, reference indices, function and call metadata, constant encodings,
and control-flow targets.  It must enforce implementation-defined resource
limits for input size, section sizes, nesting, constants, and code.

Malformed, truncated, unsupported, or incompatible artifacts must be rejected
with a Slug diagnostic in the module-loading or runtime category.  They must
never cause a host panic, unchecked allocation, out-of-bounds access, or
execution of partially validated code.  Exact diagnostic text is not a
compatibility promise unless a future artifact version says otherwise.

## Compatibility and evolution

An implementation that advertises support for an artifact version must accept
every valid artifact of that version whose language compatibility requirements
it supports.  It may reject artifacts that use optional features it does not
advertise.

Private VM bytecode may change freely.  The loader is responsible for
translating the stable `.cslug` representation into the implementation's
private representation, or for executing the stable representation directly.
Changing a private `Op` enum therefore does not change the artifact contract.

Incompatible changes require a new artifact version.  Compatible additions
require an explicitly specified extension mechanism and must preserve the
meaning of existing valid artifacts.  A compiler must not overwrite an
artifact with a different version without an explicit user request.

## Source and dependency policy

An artifact may include source text, source hashes, or source maps, but the
version-specific schema must say which are required and how they are verified.
Source locations are diagnostic metadata; their absence must not alter program
semantics.

Dependency resolution remains subject to the module rules in
[`language/runtime-requirements.md`](../language/runtime-requirements.md).  An
artifact must identify its dependencies sufficiently for a loader to detect a
missing or incompatible dependency.  It must not implicitly gain host
capabilities or bindings beyond the documented module and builtin surface.

## Implementation gate for version 1

Before version 1 is implemented, this document must gain a complete binary
schema, including the magic bytes, section encoding, canonical value encoding,
executable representation, verifier rules, feature negotiation, and test
fixtures.  The implementation must then prove round-trip loading, cross-build
compatibility, malformed-input rejection, and diagnostic behavior through
public CLI and module-loading tests.
