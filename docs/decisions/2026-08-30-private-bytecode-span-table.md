# Store private bytecode spans in a program table

## Context

Every private instruction currently owns an optional `SourceSpan`, including a
separate path string allocation for repeated source locations. The VM already
borrows spans for ordinary dispatch and only needs an owned public span when a
diagnostic, frame, throw, or suspension retains it. Stage 3 requires a compact
representation without making private bytecode layout part of the portable
`.cslug` contract.

## Decision

`Instruction` stores an optional `SpanId`. `Program` owns the corresponding
span table and interns source paths once. Source spans within a chunk are
deduplicated while it is built, then remapped into the program table when the
chunk is added. The public `SourceSpan` continues to be the diagnostic type;
its path uses the interned shared source allocation.

The VM resolves a `SpanId` to a borrowed public span for ordinary execution
and clones it only when state or an error must outlive that borrowed lookup.
`SourceId`, `SpanId`, tables, and their widths remain private VM details and
are not a `.cslug` format commitment.

## Consequences

- Repeated instructions and chunks no longer own repeated path strings or
  full spans.
- Program validation must reject an instruction whose span index is absent.
- Bytecode construction remains through `Chunk::emit_at`; direct instruction
  construction is intentionally limited to index-based spans.
- Future opcode metadata pools use the same program-owned, checked-index
  pattern.

## Migration

None for Slug source programs or their observable source-located diagnostics.
Private Rust bytecode builders use `Chunk::emit_at` as before.
