# Diagnostics have stable codes

## Status

Accepted

## Context

Diagnostic message text is not a stable contract, so a tool or agent that
wants to react to a failure must match prose. JSON diagnostics had a `kind`
only for runtime and startup failures, and interactive diagnostics used the
placeholders `source_error` and `runtime_error`. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Every diagnostic carries a stable `code`, listed in
[Diagnostic Codes](../reference/diagnostic-codes.md).

- Codes are `snake_case` names, unique across categories, so a code alone
  identifies a failure class.
- A code names a failure class, not an emission site or a message. About 160
  source error sites share 31 source codes; for example, every "expected ..."
  parse failure is `expected_syntax`.
- Source codes are a required field of `SourceError`, so a new error site
  cannot be added without choosing one. A test fails when a code is missing
  from the registry.
- Runtime codes reuse the existing runtime `kind` names. Startup codes reuse
  their `kind`, except that all usage failures share `usage`, because their
  kind names the subcommand.
- The CLI and the interactive server emit the same codes.

## Consequences

### Positive

- Tools match on codes instead of messages, so message wording can improve
  freely.

### Negative

- Renaming or splitting a code is a breaking change and needs a new record.
- Module resolution failures share the `module` code until they are
  classified.

### Neutral

The interactive server's protocol and host codes are unchanged and remain part
of its unstable protocol.

## Migration

None for source programs. Interactive clients that read `code` see specific
codes in place of `source_error` and `runtime_error`.
