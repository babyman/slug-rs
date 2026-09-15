# Pack installed bytecode

## Context

The public `Chunk`, `Instruction`, and `Op` types are useful mutable builders,
but their enum layout and inline vectors make them needlessly expensive to
retain in a reusable installed program.

## Decision

`Program::add_chunk` lowers builder instructions into a private fixed-width
`PackedInstruction` stream. Its opcode is a private tag, its operands are
three `u32` fields, and source locations remain compact `SpanId` values.
Variable metadata is owned by program-level pools. Installed programs retain
only packed chunks; the builder types are not an executable representation.

## Consequences

The installed instruction layout is 24 bytes on the supported target, instead
of the builder enum's 88 bytes. Bytecode remains in-process and unstable.
Validation continues to reject malformed builder programs before execution;
the temporary validation decode is private implementation detail.

## Migration

None. Hosts still construct `Chunk` and `Op` values, then transfer ownership to
`Program::add_chunk`.
