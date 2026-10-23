# Enum cases and struct schema fields separate with commas or newlines

## Status

Accepted

Supersedes [Enum cases and struct fields separate with commas or newlines](2026-10-16-member-separators-superseded.md)

## Context

The superseded record let any run of commas, newlines, and semicolons
separate enum cases and all struct fields, including construction and copy
fields. That followed from reusing one parser loop rather than from a language
decision, and it accepted forms nobody writes on purpose, such as
`red,, green` or `x: 1; y: 2`. Agents struggled with exactly one thing:
one-case-per-line declarations without commas. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

- Enum cases and struct schema fields are separated by a comma, one or more
  newlines, or both, with at most one comma between two members. A trailing
  comma and trailing newlines are allowed.
- Struct construction and copy fields require a comma between fields, with an
  optional trailing comma. Newlines may follow a comma or precede the closing
  brace.
- `;` does not separate members anywhere, and consecutive commas are invalid
  everywhere.
- A missing separator is `expected_syntax`: `expected comma or newline between
  enum cases`, `expected comma or newline between struct fields`, or, for
  construction and copy, `expected comma between struct fields`.
- An enum or struct field list left open at the end of input reports
  `expected }` with the safe fix that inserts it.

## Consequences

### Positive

- Declarations read naturally one member per line, where agents needed it.
- Value-level syntax stays strict, so a missing comma between construction
  fields is still caught.

### Negative

- Schema and construction field lists follow different separator rules.

### Neutral

The lexer distinguishes `;` from a newline, so member lists can accept one and
reject the other while both still separate statements.

## Migration

None for programs valid before either record. Programs that relied on the
superseded record's semicolon, repeated-comma, or newline-only construction
forms must use commas.
