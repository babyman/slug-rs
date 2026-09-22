# Immutable list views

## Context

List-pattern rest bindings and contiguous list slices previously allocated a
new vector for every logical tail. Fannkuch-redux-7 repeats that operation at
high frequency: ten local runs create 1,740,530 such tails. The allocation is
not observable in Slug, but it dominates work that does not change a value.

## Decision

The VM represents a private list value as shared immutable backing storage plus
a checked logical `[start, end)` range. Rest bindings and contiguous slices
retain that range instead of copying it. Length, indexing, iteration,
equality, rendering, and native list access operate on the logical range.

Persistent list operations remain value-producing. Append, prepend, and list
concatenation materialize their left logical input before producing a new list;
they never mutate shared backing storage. Stepped slices remain materialized
because their elements are not contiguous.

## Consequences

Views remove eager tail copying while preserving immutable aliases. The
metrics-enabled Fannkuch workload records both created views and views later
materialized by persistent updates, so retained-storage costs remain visible.

The representation is private and must not become a source-language,
native-API, diagnostic, or portable-bytecode contract. Future collection work
must preserve logical equality between a view and an independently allocated
list with the same elements.

## Migration

None. Existing Slug programs, native callers, and portable bytecode retain the
same observable behavior.
