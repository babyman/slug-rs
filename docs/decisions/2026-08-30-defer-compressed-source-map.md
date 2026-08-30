# Defer a compressed per-chunk source map

## Context

Stage 3 introduced program-owned `SpanId` metadata. The benchmark corpus now
reports the current inline span field size and the estimated size of a
per-chunk run map that stores one `(instruction index, SpanId)` pair for each
span change.

## Decision

Do not add a compressed source-map lookup layer yet. Keep the direct `SpanId`
on each instruction and retain the measurement in the VM benchmark harness.

Across the representative workloads, the estimated map reduces span metadata
by 34–59%, but only reduces total instruction storage by about 4–8%. The
extra lookup and verification paths are not justified until Stage 6 selects an
executable representation from broader layout and dispatch measurements.

## Consequences

- Source-span lookup remains direct in ordinary VM dispatch.
- The benchmark makes a future reconsideration evidence-based.
- A compact executable representation may use a per-chunk map if measured
  program shapes or its chosen instruction format make the saving material.

## Migration

None. Source diagnostics and private bytecode construction are unchanged.
