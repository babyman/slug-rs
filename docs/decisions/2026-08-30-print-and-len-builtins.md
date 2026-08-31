# Print and len builtins

## Context

The conformance fixture environment named `print` and `len` as required
builtins, but gave neither a signature nor observable behavior. The optional
`slug.builtin` declaration module documented only `println`. Implementations
could therefore disagree about newlines, accepted values, and string length.

## Decision

`slug.builtin` documents `print(...values):nil` and
`len(value:str|bytes|list|map):num`.

`print` renders its zero or more values with one ASCII space between adjacent
values and writes the result to standard output without a trailing newline.
`println` uses the same rendering and separator, then appends one newline.
Both return `nil`.

`len` accepts exactly one string, byte string, list, or map. It returns Unicode
scalar-value count for strings, byte count for byte strings, element count for
lists, and entry count for maps. Other values and invalid arity are checked
runtime errors.

## Consequences

The builtin declaration file, Language Specification, runtime requirements,
and support matrix now describe the same public surface. Implementations must
count Unicode scalar values rather than host bytes or grapheme clusters for a
Slug string. The Rust runtime still needs registrations and tests before it can
claim support for either builtin.

## Migration

None. The two functions previously had no specified or implemented behavior in
this repository.
