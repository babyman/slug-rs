# Adopt portable compiled modules

## Context

Slug needs a durable distribution and loading path for compiled modules,
including future modules written in Slug. The immediate priority is completing
the language with a Rust lexer and parser, where syntax, diagnostics, and
semantics can evolve quickly. The original VM foundation intentionally treated
its `Program`, `Chunk`, `Instruction`, and `Op` types as a compiler-private
boundary, so it made no serialized-bytecode compatibility promise.

That policy cannot provide a portable compiled module that an implementation
can save, distribute, validate, and execute independently of the compiler
process that produced it.

## Decision

Slug will define `.cslug` as a portable compiled-module format.  The format is
an external compatibility contract, separate from Rust VM internals.  A
conforming implementation will be able to save a compiled module and load an
artifact produced by another compatible implementation or release.

The detailed contract lives in
[`../reference/compiled-artifacts.md`](../reference/compiled-artifacts.md).  It requires explicit
format and language-compatibility versions, module identity and dependency
metadata, a validated executable representation, and source-location
information.  It also defines rejection behavior for malformed or incompatible
artifacts.

`Program`, `Chunk`, `Instruction`, and `Op` remain private Rust implementation
types.  A `.cslug` file must not be a serialized memory image or a direct
serialization of those types.  An implementation may lower a `.cslug` module
to private bytecode, interpret it directly, or compile it further.

No `.cslug` encoder or loader exists yet.  Until version 1 is implemented, no
on-disk byte layout, opcode numbering, or artifact version is emitted or
accepted by this repository.

The Rust front end remains the primary source implementation while the core
language is completed. A native-Slug lexer/parser is deferred until the source
language, diagnostic model, and compiler boundaries are stable enough to make
self-hosting an implementation benefit rather than a bootstrap burden.

## Consequences

- The project must specify and test the artifact contract before adding an
  encoder, decoder, CLI flag, or module loader support.
- Artifact loading is an untrusted-input boundary.  It must validate structure,
  references, resource limits, and compatibility before execution, and report
  a Slug module or runtime diagnostic rather than panic.
- A future native-Slug front end may be distributed as a `.cslug` module, but
  it is not a prerequisite for completing the current language milestones.
- A trusted bootstrap path remains necessary to load the first native front-end
  module if and when self-hosting is pursued.
- Changes to the format require a new format version or an explicitly specified
  compatible extension; private VM refactors do not.

## Migration

None.  Existing Rust bytecode is not serialized and no `.cslug` artifacts have
been released.
