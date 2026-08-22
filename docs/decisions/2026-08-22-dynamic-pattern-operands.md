# Pass dynamic pattern values as indexed VM operands

## Context

Pinned patterns such as `^expected` compare a candidate with the current value
of an enclosing binding. The compiler cannot embed that value in
`MatchPattern`: locals and captures vary per call, mutable bindings may change,
and top-level bindings may be declared before they initialize.

Future computed map-pattern keys have the same need for runtime values. Name
resolution must remain a compiler responsibility rather than teaching the VM
about source-level globals, locals, and captures.

## Decision

`TryMatch` receives an ordered count of dynamic pattern operands. Immediately
before the instruction, the compiler emits ordinary binding-load instructions
for every dynamic value in deterministic pattern traversal order.
All operands for a case are loaded once before any of that case's alternatives
are tested.

The VM removes those operands and the match subject from the operand stack,
then passes the values to the matcher. Dynamic `MatchPattern` nodes store an
index into that operand slice. A pinned node compares its candidate with the
indexed value using ordinary Slug equality and never creates a binding.

The compiler resolves pinned names in the enclosing lexical environment.
Statically declared top-level bindings use normal global-load behavior, so a
pin read before initialization follows the existing runtime error path.
Unknown names that cannot resolve to a local, capture, or declared global are
source semantic errors.

## Consequences

- Pinned comparisons observe the current value of mutable bindings.
- The matcher remains independent of source names and lexical storage classes.
- Alternatives and failed nested patterns continue to use the existing
  binding-checkpoint rollback.
- Computed map keys can reuse the operand channel without another VM/source
  name-resolution boundary.
- Internal bytecode constructors and direct VM tests must provide the operand
  count, even when it is zero.

## Migration

No Slug source migration is required. `Op` and `MatchPattern` are private
implementation bytecode and may change; Rust tests constructing them directly
must adopt the new `TryMatch` field.
