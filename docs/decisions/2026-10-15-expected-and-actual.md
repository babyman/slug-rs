# Diagnostics report what was expected and found

## Status

Accepted

## Context

A type or syntax error's message embeds what was required and what was found,
such as `expected num, got str`. A tool had to parse the message to recover
them. See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Diagnostic documents carry `expected` and `actual` members, defined in
[Diagnostic Documents](../reference/diagnostics.md), and `SourceError` carries
the same fields.

- A parse error raised at a token reports that token as `actual`. When the
  parser required one specific token, it reports that token as `expected`.
- A type mismatch against one required type reports both types.
- Both are descriptive text, not stable identifiers. `code` remains the stable
  key.
- Errors that do not know one side, such as operator errors whose requirement
  is not one type, report `null` rather than a guess.

## Consequences

### Positive

- Tools can show or act on the mismatch without parsing message text.

### Negative

- Coverage is partial; most semantic codes other than `type_mismatch` report
  `null`.

### Neutral

Tokens describe themselves through one exhaustive match, so a new token kind
cannot be added without a description.

## Migration

None.
