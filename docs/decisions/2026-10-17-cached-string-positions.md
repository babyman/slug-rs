# Strings cache their length and character offsets

## Status

Accepted

## Context

String positions count Unicode scalar values, but strings are stored as UTF-8.
Every index counted the whole string to resolve negative positions and check
bounds, then walked to the position, and `len` and slicing counted it too. Any
program walking a string with `text[i]` was quadratic. A JSON parser written
that way took 3.72 s on a 130 KB document; an agent writing the same library
noticed the cost and converted the text to a list first. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

- A string keeps its scalar count beside its text. `+` appending in place
  updates the count from the appended text alone.
- When the count equals the byte length, the text is ASCII and scalar
  positions are byte positions, so indexing and slicing are constant-time.
- Otherwise the first positional access builds a table holding the byte offset
  of every 64th scalar, so a lookup scans at most 64 characters. Appending
  discards the table.
- `slug.string` adds `chars(text)`, returning the characters as a list of
  one-character strings, for programs that iterate or match over characters.

This is a representation change only. String semantics, including scalar
positions and their errors, are unchanged.

## Consequences

### Positive

- Ordinary character loops are linear without workarounds.

### Negative

- Each string carries a count and an empty table cell, and an indexed
  non-ASCII string also carries its offset table.
- Creating a string counts its scalars once.

### Neutral

The interleaved benchmark corpus is neutral on tuned workloads and slightly
faster on hold-outs; see the
[VM optimization record](../planning/vm-optimization.md#cached-string-positions).

## Migration

None.
