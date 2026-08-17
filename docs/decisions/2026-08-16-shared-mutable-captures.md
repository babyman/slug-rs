# Share mutable captures through binding cells

## Context

The core Rust frontend added lexical `var` bindings and closures, but initially
captured values by copying them into each closure.  That made an assignment to
a captured `var` impossible and violated the language requirement that ordinary
closures share captured mutable bindings.

## Decision

Every VM frame-local binding is stored in a reference-counted binding cell.
When a closure captures a local or an enclosing capture, it retains that cell;
it does not copy the current value.  `GetLocal`, `SetLocal`, `GetCapture`, and
`SetCapture` read or replace the cell value.  Immutable source bindings remain
protected by the compiler, while mutable bindings may be assigned through any
closure that captures them.

The cells are a VM-internal representation.  They are not Slug values, are not
serialized, and make no claim about the future `.cslug` encoding.

## Consequences

- Repeated calls to a counter closure observe prior assignments.
- Sibling closures created in one lexical scope observe the same mutable
  binding.
- Nested captures preserve cell identity through intermediate closures.
- All local bindings currently use cells for a simpler checked VM invariant;
  implementations may later optimize immutable unshared locals without
  changing observable behavior.

## Migration

Programs that previously received a semantic rejection for assignment to a
captured `var` now execute according to shared lexical-binding semantics.
