# Retain an unstable Rust bytecode surface

## Context

Slug documents its bytecode as private because it is not a portable format or a
source-language compatibility promise. At the same time, the crate publicly
exports `Program`, `Chunk`, `Instruction`, `Op`, and their construction
metadata. Public `compile` APIs also return `Program`, while integration tests
construct programs directly to prove verifier and checked-runtime behavior.

Calling those types private while exporting them makes the crate boundary
unclear. Making them crate-private would require a replacement Rust compilation
and execution API as well as migration of the direct-bytecode test boundary.

## Decision

`Program`, `Chunk`, `Instruction`, `Op`, and their supporting metadata remain
public as an in-process Rust embedding and testing surface during pre-release
development.

This surface permits Rust callers to construct a program, run it through `Vm`,
and receive checked failures for malformed programs. It is explicitly unstable:
its layouts, variants, constructors, and semantics may change without Rust API
compatibility. It must not be serialized, exchanged between versions, or used
as a portable module representation.

The future `.cslug` format remains the only portable compiled-module contract.
It must translate into an implementation representation rather than encode
these Rust types directly. This clarifies, but does not supersede, [Portable
compiled modules](2026-08-16-portable-compiled-modules.md).

## Consequences

Direct bytecode integration tests remain the verification boundary for malformed
programs and VM behavior. Rust hosts may use the surface for experimental
embedding without relying on its shape across pre-release versions.

Documentation must distinguish Rust visibility from compatibility: public does
not mean stable or portable. A future stable embedding API requires a separate
decision and migration plan; it must not be created accidentally by preserving
these types indefinitely.

## Migration

None. Existing direct-bytecode callers continue to compile subject to ordinary
pre-release API changes, and no source-language or `.cslug` migration occurs.
