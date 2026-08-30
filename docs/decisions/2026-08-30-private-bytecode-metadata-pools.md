# Pool variable private-bytecode operands

## Context

Private instructions still embed strings, patterns, capture lists, schema
fields, and struct field lists. These values make instruction size depend on
host collection layouts and duplicate metadata across instructions.

## Decision

`Chunk` continues to accept the existing ergonomic build-time opcode forms.
When `Program::add_chunk` takes ownership of a chunk, it moves global names,
match patterns, capture lists, schema fields, and struct field lists into
program-owned pools and rewrites the instruction to typed metadata indices.
The VM and verifier operate on the pooled forms; an unpooled form is invalid
inside an installed program.

Pool indices are checked before execution. They are private Rust bytecode
metadata, not an opcode encoding or `.cslug` compatibility contract.

## Consequences

- Source compilation and focused bytecode tests retain simple construction
  syntax.
- Runtime dispatch borrows pooled metadata rather than cloning variable-size
  operands from instructions.
- Each new pool needs a malformed-index verifier test.
- Other variable-size opcode operands remain a later Stage 3 slice.

## Migration

None for Slug source programs. Direct private-bytecode consumers construct
chunks before adding them to a program, as before.
