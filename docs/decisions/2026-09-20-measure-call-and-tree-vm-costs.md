# Measure call and tree VM costs before changing representations

## Context

Changing the release profile from size optimization to `opt-level = 3` made
the current private stack VM substantially faster in end-to-end source runs.
The 2026-09-20 `make bench-source` run leaves two materially slower workloads:
the repeated ordinary-call program and the binary-tree construction and match
program. Existing VM counters explain ordinary call setup, but do not measure
the source-aligned binary-tree workload or distinguish packed direct dispatch
from rich-op fallback dispatch.

Frames own the lexical global environment so imported and retained interactive
closures execute and suspend in the caller's VM. A hot-path optimization must
not weaken that ownership rule or its cleanup and diagnostic behavior.

## Decision

Optimize calls and binary trees through measured, independent slices before
changing the operand model, language semantics, or portable `.cslug` contract.

1. Add source-aligned in-process workloads for `function-call` and
   `binary-trees`. Record frames, local-vector capacity, collection
   construction, and instructions dispatched directly from packed bytecode
   versus through the rich-op fallback.
2. Instrument `Value` cloning and reference-count traffic only for the
   binary-tree experiment if the first measurements cannot distinguish list
   construction from match or call cost. Do not make those counters permanent
   broad runtime instrumentation.
3. Measure moving the VM's current-global synchronization from every
   instruction fetch to frame push, pop, recovery, and other frame-transition
   boundaries. The active frame remains the authority for lexical globals.
4. If local-vector allocation remains material, compare a VM-local vector
   recycler with the existing planned contiguous local-slot-arena prototype.
   Retain only a representation that preserves captured-cell identity across
   `recur`, closure escape, cleanup, suspension, and cross-program calls.
5. Consider a two-element-list fast path or packed match dispatch only when
   the source-aligned binary-tree measurements identify that cost as dominant.

Type-informed numeric lowering is not part of this round. The paired typed
numeric source workloads remain its evidence gate.

## Consequences

The next implementation tasks have explicit workload and correctness gates,
so an allocation, dispatch, or operand-model rewrite is not selected from
general timing alone. The VM continues to expose checked failures and source
locations, and cross-program closure behavior remains frame-owned.

This adds narrowly scoped benchmark and metric maintenance before a runtime
representation change. A candidate that does not improve its targeted
measurements, or broadens the private-bytecode compatibility surface, is not
retained.

## Migration

None. This decision changes only private VM implementation and measurement
planning. Slug syntax, semantics, diagnostics, `docs/language/`, and
`docs/language/slug.ebnf` are unchanged.
