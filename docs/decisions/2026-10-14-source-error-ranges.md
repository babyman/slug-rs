# Source errors have ranges

## Status

Accepted

## Context

Diagnostics located a failure by one point. A tool that highlights the failure
or proposes an edit needs its extent. Point locations are often focus points
rather than starts: a binary operator error points at the operator. Runtime
locations come from bytecode span tables that `.cslug` stores as points. See
the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Parse and semantic errors carry an optional `range` from a start position to
an exclusive end, alongside the unchanged point `location`.

- Tokens record their end. A parse error at a token covers that token; a lexer
  error covers the source consumed up to the failure.
- Every syntax-tree expression records its end, taken from the last token the
  parser consumed. Its start is computed when needed as the leftmost position
  in its subtree, so a range covers operands left of a focus point.
- An error raised without a range at an expression's span takes that
  expression's extent as it propagates out of the checker, validator, or
  lowering. The innermost expression with the error's span claims it, so
  helpers that receive only a span need no change.
- `slug-vm`'s `SourceSpan` is unchanged. Runtime errors keep point locations
  and `range` is null for them.

## Consequences

### Positive

- Tools can highlight or replace the offending source without re-parsing it.

### Negative

- Errors raised away from any expression, and a few raised at spans that no
  expression owns, still have no range.
- A range is only as accurate as its error's span. An error reported at the
  wrong expression covers that expression.

### Neutral

Runtime ranges remain future work and require a `.cslug` span-table change.

## Migration

None.
