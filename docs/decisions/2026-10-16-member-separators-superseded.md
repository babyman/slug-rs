# Enum cases and struct fields separate with commas or newlines

## Status

Superseded by [Enum cases and struct schema fields separate with commas or newlines](2026-10-16-declaration-member-separators.md)

## Context

Enum cases and struct fields required commas. One-per-line declarations without
commas, the form [Nominal Types](../language/nominal-types.md) already showed
for enums, failed with `expected } after enum cases`. Every agent run of the
evaluation's enum task hit that error. Newlines already separate statements
and match cases.

## Decision

Enum cases, struct schema fields, and struct construction and copy fields are
separated by any run of commas and newlines, or `;`, with an optional trailing
separator. Members with no separator between them are a parse error with code
`expected_syntax`: `expected comma or newline between enum cases` or
`expected comma or newline between struct fields`. An enum or struct field
list left open at the end of input reports `expected }` and carries the safe
fix that inserts it.

## Consequences

### Positive

- The documented one-case-per-line enum form now parses, and declarations
  read the same with or without commas.

### Negative

- Repeated commas, such as `red,, green`, are accepted.

### Neutral

The grammar adds one `member_sep` rule shared by enums and structs.

## Migration

None. Every previously valid program remains valid.
