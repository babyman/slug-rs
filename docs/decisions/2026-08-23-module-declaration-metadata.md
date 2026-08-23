# Retain evaluated module declaration metadata

## Context

Top-level documentation and tags are part of the source language, and future
`slug.meta` introspection needs their evaluated values after module execution.

## Decision

Compiled programs retain static top-level declaration metadata. A private VM
instruction records each declaration tag's evaluated arguments while the
module initializes. Completed module instances retain the resulting metadata.

## Consequences

- Tag argument evaluation remains source ordered and happens exactly once.
- The module model can support `slug.meta` without rerunning source code.
- No Slug-level metadata API is exposed until the public library milestone.

## Migration

None.
