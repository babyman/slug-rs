# Select overloads from static callable signatures

## Context

Imported modules can combine distinct callables under one name. The current VM
stores those callables in an overload set, tries them in import order, and
selects the first candidate whose positional, named, default, and variadic
argument binding succeeds. The binding metadata does not retain parameter
annotations. Consequently, two same-shape overloads such as `render(str)` and
`render(bytes)` cannot be distinguished, and an import-order implementation
detail can select an unrelated function.

The optional type checker can validate calls to one directly known annotated
function, but represents only one function per name. It neither retains an
overload set nor preserves exported signatures for an importing module. Making
the VM inspect annotation types would make annotations an implicit runtime
contract, conflict with Slug's dynamic value model, and duplicate generic
inference in the execution path.

Imported bindings are live. A compiled call therefore cannot permanently
embed a closure or bytecode-chunk target: an exported `var` may later replace
the callable bound to its name. This record clarifies the callable-set
architecture anticipated by [Bind imported closures to their defining
module](2026-08-23-module-closure-contexts.md) and extends the private
binding metadata adopted by [Keep callable signatures in private bytecode
metadata](2026-08-22-private-call-signatures.md). It uses the top-type and
inference rules adopted by [Make `any` the non-nil top
type](2026-08-28-non-nil-any-type.md).

## Decision

### Callable sets are semantic metadata

The source semantic model represents a statically known callable as an ordered
set of callable signatures, rather than a single function type. Local
declarations, `foreign` declarations, and module exports all use that model.
An exported module exposes a cached semantic snapshot of its exported callable
sets to importers; import sites must not reconstruct signatures from call
usage or re-analyze module source.

A signature contains its type-parameter arity and its parameter call labels,
annotations, default-presence, and final-variadic flag. Call labels are part of
the signature because named calls can observe them; a discard parameter has no
call label. The result annotation is retained alongside the signature for
result inference and checking, but it is not part of overload identity or
selection. This semantic metadata is private implementation data, not portable
bytecode or a `.cslug` compatibility promise.

### Overloaded calls select one signature statically

Type annotations do not introduce runtime validation or coercion. Parameter
annotations participate in mandatory resolution of statically known overloads.
Optional type checking uses annotations for additional diagnostics; it never
changes which signature a program accepted in both modes selects.

Whenever the callee expression has a statically known callable set, semantic
validation performs argument-shape binding and generic inference for every
candidate before compilation. A candidate is applicable only when its named,
positional, default, variadic, and explicit type-argument rules succeed and
all known argument types satisfy its instantiated parameter annotations.
The compiler trusts a binding's declared annotation for this purpose even when
optional type checking is disabled.

Unknown is a transient inference state, not a source type. If an argument's
type remains unknown after inference, it widens to `any|nil` before overload
selection. A narrow candidate is therefore inapplicable unless the argument is
narrowed or annotated; selection never guesses a narrow overload from missing
type information.

The call must have one most-specific applicable candidate. A candidate is more
specific when its instantiated parameter types accept a strict subset of the
values accepted by another applicable candidate, with the same bound call
shape. If no candidate applies, compilation reports a source diagnostic. If
multiple candidates remain equally specific or incomparable, compilation
reports an ambiguous-overload source diagnostic. Declaration or import order
must not break either tie.

An unannotated parameter has type `any|nil` for this comparison. It may provide
a broad fallback, but it must not hide a more-specific applicable candidate.
Union and generic signatures use the same instantiated assignability relation
as ordinary static call checking; no runtime coercion is introduced.

The selected candidate's result annotation determines the static type of the
call after selection. An expected result type does not make a candidate
applicable or more specific, so declarations that differ only in their result
annotations do not form return-type-directed overloads.

Calls through values whose callable set is not statically known retain ordinary
dynamic-call behavior. Computed lookups, dynamic imports, and otherwise
untyped `fn` values therefore remain callable, but cannot receive type-aware
overload selection. Their runtime argument binder continues to enforce only
the existing call-shape rules.

### Lower a selected signature, not a fixed function target

The compiler records the selected private signature identity at a statically
resolved overloaded call. At execution, the VM reads the current callee value
from its normal binding, verifies that the selected signature is still present
in that callable set, and invokes that member using the ordinary shared
argument binder. The VM does not compare runtime values with annotation types
or search candidates by type.

If a live binding no longer contains the selected signature, execution fails
with a checked call error at the call span. It never falls back to a different
overload. This guard preserves live-binding semantics without allowing a
runtime replacement to silently change which overload the source call means.

### Signature identity governs overload merging

Callable identity uses canonical structural equality rather than source-text
equality or mutual assignability. The canonical input signature applies these
rules recursively:

- Type parameters are identified by declaration-order ordinal, and generic
  arity is retained. Renaming `T` to `U` does not change identity, while using
  the first and second type parameters in different positions does.
- An unannotated parameter canonicalizes to `any|nil`, so it is identical to an
  explicit `any|nil` parameter.
- Nested unions are flattened, duplicate members are removed, and members are
  placed in canonical order. Tuple elements and type-application arguments
  retain their declared order.
- Each parameter retains its call-visible label, default-presence, and variadic
  flag. A discard parameter has no label. The default expression itself does
  not participate.
- Result annotations, documentation, tags, source locations, local binding
  names that are not call-visible, and implementation targets do not
  participate.

Assignability determines candidate applicability and specificity, never
signature equality. Module import merging, local duplicate-callable
diagnostics, and live-binding signature lookup all compare the same canonical
input structure. Distinct signatures form an overload set. Identical local
signatures are duplicate declarations; identical imported signatures retain
the established first-loaded binding and warning. An implementation may hash
the canonical structure for lookup, but the hash is not its semantic identity.

## Consequences

- Same-shape overloads can differ by declared parameter types without relying
  on import order at calls that have known argument types.
- Local and imported calls share generic inference, nilability, named-argument,
  and ambiguity rules.
- Return annotations remain available for checking and inference, but cannot
  distinguish otherwise identical callable declarations.
- The compiler, module metadata, and VM need a shared private signature
  identity and tests for both local and imported callable sets.
- Alpha-renaming generic parameters and reordering union members do not change
  signature identity; changing a call label, default-presence, or variadic
  status does.
- Static selection is a mandatory semantic compilation rule for known callable
  sets. The optional `-type-check` mode may reject additional inconsistencies,
  but cannot change overload selection for a program accepted in both modes.
- Parameter annotations have compile-time operational meaning for overload
  resolution even when optional type checking is disabled.
- Dynamic function values remain supported, but they deliberately retain
  shape-based runtime dispatch and cannot promise typed overload selection.
- The VM avoids a second type system and annotations remain non-coercive.

## Migration

Existing calls whose overload selection depended on import order will either
resolve to their uniquely most-specific signature or fail at compilation as
ambiguous. Module authors should add parameter annotations, use distinct names,
or make the call's argument type explicit to remove ambiguity. Adding or
changing a parameter or binding annotation can change static overload
selection even without `-type-check`. No source syntax or portable-bytecode
migration is introduced.
