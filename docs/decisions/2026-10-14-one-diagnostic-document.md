# The CLI and the interactive server share one diagnostic document

## Status

Accepted

## Context

The command line's `--diagnostic-format=json` documents and the interactive
server's diagnostics described the same failures differently. The command line
used the categories `parse`, `semantic`, `module`, `runtime`, `usage`, `io`,
`host`, and `artifact` and wrote absent members as `null`; the server used
`source`, `runtime`, `protocol`, and `host`, put the source stage in `kind`,
omitted absent members, and alone reported native error details. A tool that
consumed both had to handle two schemas. Stable codes made `kind` redundant.
See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Both emit one version 2 document, defined in
[Diagnostic Documents](../reference/diagnostics.md) and produced by one Rust
type, `slug::diagnostic::Diagnostic`.

- `category` uses the command line's stage names plus `protocol`.
- `kind` is removed. `code` names the failure class and `category` the stage.
- Every member is always present and `null` when it does not apply, except
  `thrown`, whose presence distinguishes `throw nil` from no payload.
- The command line now reports `native` details.

## Consequences

### Positive

- One document, one schema, and one implementation for both surfaces. A test
  checks that both emit the same document for the same failure.

### Negative

- Version 1 consumers of `--diagnostic-format=json` must read `code` instead
  of `kind`.

### Neutral

The REPL renders `<category> error (<code>)`, such as
`semantic error (type_mismatch)`.

## Migration

Command-line consumers move from version 1 to version 2: read `code` instead
of `kind`. Interactive consumers read `parse` or `semantic` from `category`
instead of `source` plus `kind`.
