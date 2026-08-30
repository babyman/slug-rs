# Select a future compact executable shape

## Context

The typed private `Instruction` representation is 64 bytes on the current
host. The benchmark corpus now reports its largest chunk, constant pool, local
frame, and metadata pool so a compact representation can be chosen from data
rather than Rust enum layout or pointer width.

## Decision

If a separate installed executable encoding is introduced, it uses an 8-bit
opcode tag and 32-bit operand and metadata-index fields. These widths are
private implementation choices and are not a `.cslug` format.

Keep the current checked typed representation as the executable form for now.
The measured corpus does not justify a second byte-stream lowering before the
Stage 7 stack/register comparison. Retain regular stack operations for simple
loads, arithmetic, comparisons, jumps, and returns. Retain calls, closures,
collections, matching, cleanup, throwing, and recurrence as medium-grained
semantic operations. Do not add a fused superinstruction without a measured,
recurring hot sequence.

## Consequences

Future encoding work has fixed field-width guidance without making the host
layout or private opcode values a compatibility promise. The verifier and
compiler avoid a premature duplicate encoding layer, and Stage 7 can compare a
register prototype fairly against the optimized typed stack VM.

## Migration

None. Slug source, private-bytecode construction, and `.cslug` remain
unchanged.
