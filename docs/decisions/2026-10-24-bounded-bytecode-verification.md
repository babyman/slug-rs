# Bound bytecode verification and analysis

## Status

Accepted

## Context

[`.cslug` artifacts](2026-10-04-vm-first-whole-program-cslug.md) are untrusted
input, and decoding must return a checked error for any malformed or hostile
file. Three installation steps did not bound their memory by the artifact's
size:

- A chunk's arity and local count were read as plain integers. Last-read
  analysis then allocated one local-slot bitset per instruction, so a
  193-byte artifact with an enormous local count aborted `slug report` and
  `slug run` on a failed multi-exabyte allocation.
- Even with bounded counts, that analysis needs instructions × locals / 64
  words, about 128 GB for a million of each.
- The verifier kept a full copy of the operand stack at every instruction and
  copied it at each step, so a chunk that pushes a million values needed
  quadratic memory and time.

Last-read analysis also ran before verification, and the verifier accepted
backward jumps, so the analysis's fixed point could take many passes.

## Decision

- Arity and local count are table counts, bounded by
  `Limits::entries_per_table` (1,048,576 by default) before anything is
  allocated for them.
- Verified bytecode jumps only forward: a `Jump`, `JumpIfFalse`,
  `JumpIfFalsePop`, or `JumpIfProvided` whose target is at or before its own
  instruction is invalid. The compiler emits only forward jumps; loops restart
  through `recur`, which verification treats as a return to instruction 0
  with the entry state.
- The verifier tracks each instruction's operand-stack height and scope
  depth, not the stack's contents, in one pass in instruction order. Where
  paths join, the lowest height is kept, as before.
- Decoding marks last local reads only after verification. The analysis
  first clears every move flag, including flags an artifact supplies, and
  skips a chunk whose bitsets would exceed 32 MiB; that chunk's reads all
  copy their locals, which is always correct.

## Consequences

Decoding and verification take memory and time linear in the artifact, apart
from the analysis's fixed budget per chunk. A chunk with a local count at the
limit still allocates that many slots per call when it runs; running a hostile
program is not made safe by decoding it safely.

A future instruction that needs a backward jump must change this rule and the
analyses that rely on it together.

## Migration

None for source programs. An artifact with a backward or self jump, which the
compiler never emits, is now rejected.
