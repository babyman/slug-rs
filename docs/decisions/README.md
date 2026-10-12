# Decision Records

Decision records preserve decisions that a future implementation task cannot
infer reliably from code and specifications alone.

Create a record for a non-trivial decision involving language syntax or
semantics, runtime or bytecode architecture, public diagnostics, or a
compatibility promise. Do not create one for formatting, mechanical renames, or
a local correction with an established rule.

Place current records at `YYYY-MM-DD-short-title.md`. Records are immutable
after their decision is implemented, except for status and cross-reference
metadata. When replacing a decision, rename the older record to
`YYYY-MM-DD-short-title-superseded.md`, mark it `Superseded`, and link to the
replacement. Mark the replacement `Accepted` and link back with `Supersedes`.
Files ending in `-superseded.md` or `-withdrawn.md` are historical and are not
current architectural guidance.

Use this structure:

```md
# Title

## Context

What problem, source rule, or architectural pressure requires a decision?

## Decision

What exact rule or design is adopted?

## Consequences

What becomes simpler, what is intentionally unsupported, and what code,
tests, or documentation must remain aligned?

## Migration

How existing source programs, bytecode, or tests are affected. Use `None` when
there is no migration.
```
