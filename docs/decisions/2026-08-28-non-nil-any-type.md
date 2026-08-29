# Make `any` the non-nil top type

## Context

Slug needs to distinguish a value known to be non-nil, a value that may be
nil, and a type the semantic analyzer has not inferred yet. Treating an unknown
type name as unconstrained hides annotation mistakes and makes `any|nil`
indistinguishable from `any`. Persisting an internal unknown type in callable
metadata would also make imported checking and overload selection depend on
missing information.

Unannotated parameters accept every Slug value, while omitted binding and
result annotations can often be inferred more precisely. These cases need
different rules even though all of them previously lacked source annotations.

## Decision

### `any` excludes `nil`

`any` is the built-in top type for non-nil Slug values. Every non-nil type is
assignable to `any`; `nil` is not. `any|nil` is the universal value type and
accepts every Slug value. A function returning `any` therefore promises a
non-nil result, while a function returning `any|nil` may return nil.

Union normalization treats `any` as absorbing every non-nil member. Thus
`any|str` normalizes to `any`, and `any|nil|str` normalizes to `any|nil`.
`any` and `nil` remain distinct canonical members.

Unresolved annotation names are source errors. The semantic analyzer uses a
private unknown state while inference is incomplete, but unknown is not a
source type, an exported type, or a callable-signature identity.

### Omitted annotations infer or widen

An unannotated parameter accepts every value and has the canonical type
`any|nil`. Consequently, `fn(value)` and `fn(value:any|nil)` have identical
input signatures and cannot form overloads.

An unannotated binding is inferred from its initializer. An unannotated
function result is inferred from all reachable result expressions. If analysis
cannot determine a more precise binding or result type, the private unknown
state widens to `any|nil` before the type is retained in module metadata or
used for overload resolution.

An explicit annotation defines the retained public type even when the
initializer or function body has a narrower inferred type. For example,
`fn():any|nil { 1 }` exposes `any|nil`, while `fn() { 1 }` infers `num`.

### Bare generics remain non-nil

A bare generic parameter ranges over non-nil types. It cannot be inferred as
`nil` or explicitly instantiated with a type containing nil. A declaration
uses `T|nil` when its input or result may be nil. A call that supplies only nil
to a `T|nil` inference position does not establish `T`; another argument or an
explicit non-nil type argument must do so.

## Consequences

- `any` expresses a non-nil guarantee without runtime validation or coercion.
- `any|nil` is the safe final type for values whose type remains unknown after
  inference.
- Static overload resolution cannot select a narrow overload from a genuinely
  unknown value merely because that overload is more specific.
- Local and imported callable metadata never need to preserve an unknown type.
- Standard-library declarations that use `any` must state nilable results
  explicitly and should preserve generic payload types where possible.

## Migration

Annotations that relied on an unrecognized name behaving as unconstrained must
use `any`, `any|nil`, or a declared generic parameter. Calls that infer a bare
generic parameter solely from nil become source errors. Unannotated parameters
and explicitly `any|nil` parameters become duplicate input signatures. No
source syntax or runtime-value migration is introduced.
