# Check statically known expression operations

## Context

Slug's optional checker preserves types through declarations, collections,
function values, and match constraints, but most ordinary operations merely
return a guessed result type. A program such as `"name" - 1` is therefore
accepted with `-type-check` even though both operands are statically known to
be invalid for subtraction. Conversely, the checker must not turn dynamic
programs into rejected programs merely because their value type is unknown.

## Decision

With `-type-check`, check an operation when its operand types prove a single
supported operand family. Numeric arithmetic, bitwise and shift operators,
negation, ordering comparisons, list append/prepend, string concatenation and
repetition, list concatenation, indexing, and list slicing use their runtime
operand families. Equality and logical operators remain valid for every type.

`unknown`, `any`, and unions that include either remain dynamic fallbacks. A
fully known incompatible operand or union is a semantic error. Successful
operations retain their precise result types: list access yields its element
type, map access yields its value type plus `nil`, list slices retain their
element type, list concatenation and directional operations union their
element types, and string operations yield `str`.

This decision does not add flow-sensitive narrowing, field-level schema
metadata, coercion, or runtime checks. Static checking continues to diagnose
only in `-type-check` mode.

## Consequences

- Existing annotations become useful for ordinary expressions without changing
  untyped runtime behavior.
- The checker may reject known incorrect operations before execution while
  preserving dynamic behavior when available type information is incomplete.
- `num` is intentionally broader than the VM's integer-only bitwise, shift,
  index, and slice-bound operations. The checker accepts `num` there rather
  than inventing an integer source type; a non-integral dynamic number retains
  the established runtime error.

## Migration

Programs run with `-type-check` may need to correct operations that were
already runtime type errors. Programs without that flag are unchanged.
