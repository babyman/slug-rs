# Versioned JSON CLI diagnostics

## Context

Human-readable command-line diagnostics include source excerpts and carets,
which are useful interactively but require agents and editor integrations to
parse presentation text. The runner already retains structured source spans,
runtime fault kinds, frames, and error causes.

## Decision

`slug --diagnostic-format=json program.slug` writes one versioned JSON
diagnostic to standard error for each runner-generated fatal failure. The flag
is recognized only before the entry program, preserving the existing
post-program argument boundary. Version 1 contains category, optional subtype,
message, optional location, runtime frames, and recursive cause. It does not
serialize arbitrary Slug values or native error data.

## Consequences

The JSON schema is a public runner compatibility surface and must be changed
only through a new version. Human-readable diagnostics remain the default.
Tests must preserve stderr-only JSON output, exit status, locations, frames,
and the argument boundary.

## Migration

None. Existing invocations retain their current text diagnostics and program
arguments.
