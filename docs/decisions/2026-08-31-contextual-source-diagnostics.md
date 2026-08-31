# Contextual Source Diagnostics

## Context

Path, line, and column identify a source failure but force a command-line user
to reopen the source and count to the reported position. The CLI already has
structured source-error spans, although the source associated with a span is
not guaranteed to remain available.

## Decision

The CLI renders parse, semantic, and runtime errors with a short source excerpt
and a caret at the reported position when it can obtain the referenced source
text. It uses the already-loaded entry source when applicable and otherwise
makes a best-effort read of the span path. If the source, reported line, or
span is unavailable, it preserves the existing one-line location diagnostic.

## Consequences

Diagnostic rendering is a CLI presentation concern; `SourceError` remains a
structured API containing only its category, message, and optional position.
Runtime call frames follow the contextual excerpt. The renderer expands tabs
consistently in both the displayed source and caret padding. Exact fixture
diagnostics may opt into either permitted rendering.

## Migration

None. Existing programs retain the same failure categories and locations.
