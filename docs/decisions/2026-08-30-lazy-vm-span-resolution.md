# Resolve VM spans only at ownership boundaries

## Context

Private instructions store `SpanId`s, but the VM resolved the source table at
the start of every dispatch iteration even when the instruction completed
without needing a source location. The ordinary path therefore paid a table
lookup for arithmetic, collection, and control-flow instructions solely to
forward an optional borrowed span to later error handling.

## Decision

The VM retains the current instruction's `SpanId` during dispatch. It resolves
that ID only when constructing a diagnostic or storing source information in a
call frame, task, or suspended select state. Opt-in metrics count those
ownership-boundary source-table lookups and owned-span clones.

## Consequences

Successful instructions with no durable source owner avoid a source-table
lookup. Checked diagnostics retain their original locations because the active
ID is resolved before constructing the owned error. Metrics must continue to
assert both zero-lookups for ordinary execution and one lookup for each
intentional durable or diagnostic boundary.

## Migration

None. `Program`, `Instruction`, and `SpanId` remain private bytecode details.
