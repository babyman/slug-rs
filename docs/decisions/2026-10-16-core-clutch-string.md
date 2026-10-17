# The core clutch provides slug.string with ord and chr

## Status

Accepted

## Context

Slug strings can be indexed and sliced by Unicode scalar value, but nothing
converts a character to its scalar value or back. Code that encodes or
decodes text, such as a JSON library handling `\u` escapes, cannot be written
without that conversion. The core clutch already provides `slug.std`,
`slug.io.stdin`, and `slug.math`, with `slug.math` wrapping typed native
bindings in Slug functions that throw typed errors. See
[the core clutch's math module](2026-09-10-core-clutch-math.md).

## Decision

The core clutch adds a `slug.string` module, the start of Slug's string
library, with two exported functions.

- `ord(character:str):num` returns the Unicode scalar value of a string of
  exactly one character.
- `chr(code:num):str` returns the one-character string for a Unicode scalar
  value: an integer from 0 through 0x10FFFF that is not a surrogate.
- Invalid arguments throw `Error` values whose `type` is `string.range`.
- Each wraps a typed foreign binding implemented in the clutch's C library,
  as `slug.math` does, rather than adding a VM builtin. The wrappers validate
  arguments in Slug, so the native functions only encode and decode UTF-8.

## Consequences

### Positive

- Text encoders and decoders, starting with a JSON library, can be written in
  Slug.
- String functions grow in a library module, outside the language core.

### Negative

- Programs must import `slug.string`, and hosts without the core clutch's
  native library do not provide it.

### Neutral

`chr` returns its result through a per-thread buffer that the typed binding
copies before the next call.

## Migration

None.
