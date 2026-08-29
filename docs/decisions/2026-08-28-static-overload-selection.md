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
metadata](2026-08-22-private-call-signatures.md).

## Decision

### Callable sets are semantic metadata

The source semantic model represents a statically known callable as an ordered
set of callable signatures, rather than a single function type. Local
declarations, `foreign` declarations, and module exports all use that model.
An exported module exposes a cached semantic snapshot of its exported callable
sets to importers; import sites must not reconstruct signatures from call
usage or re-analyze module source.

A signature contains its ordered type parameters and its parameter names,
annotations, default-presence, and final-variadic flag. Parameter names are part
of the signature because named calls can observe them. The result annotation is
retained alongside the signature for result inference and checking, but it is
not part of overload identity or selection. This semantic metadata is private
implementation data, not portable bytecode or a `.cslug` compatibility promise.

### Overloaded calls select one signature statically

Whenever the callee expression has a statically known callable set, semantic
validation performs argument-shape binding and generic inference for every
candidate before compilation. A candidate is applicable only when its named,
positional, default, variadic, and explicit type-argument rules succeed and
all known argument types satisfy its instantiated parameter annotations.

The call must have one most-specific applicable candidate. A candidate is more
specific when its instantiated parameter types accept a strict subset of the
values accepted by another applicable candidate, with the same bound call
shape. If no candidate applies, compilation reports a source diagnostic. If
multiple candidates remain equally specific or incomparable, compilation
reports an ambiguous-overload source diagnostic. Declaration or import order
must not break either tie.

An unannotated parameter is unconstrained for this comparison. It may provide a
broad fallback, but it must not hide a more-specific annotated candidate. Two
applicable unconstrained candidates remain ambiguous. Union and generic
signatures use the same instantiated assignability relation as ordinary static
call checking; no runtime coercion is introduced.

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

Module import merging and duplicate-callable diagnostics compare the complete
input signature, including type parameters, parameter names, annotations,
default-presence, and the final-variadic flag, rather than the current
default/variadic shape alone. Result annotations do not participate. Distinct
signatures form an overload set; identical signatures retain the established
first-loaded binding and warning.

## Consequences

- Same-shape overloads can differ by declared parameter types without relying
  on import order at calls that have known argument types.
- Local and imported calls share generic inference, nilability, named-argument,
  and ambiguity rules.
- Return annotations remain available for checking and inference, but cannot
  distinguish otherwise identical callable declarations.
- The compiler, module metadata, and VM need a shared private signature
  identity and tests for both local and imported callable sets.
- Static selection is a semantic compilation rule for known callable sets; it
  is not controlled by the optional `-type-check` diagnostic mode.
- Dynamic function values remain supported, but they deliberately retain
  shape-based runtime dispatch and cannot promise typed overload selection.
- The VM avoids a second type system and annotations remain non-coercive.

## Migration

Existing calls whose overload selection depended on import order will either
resolve to their uniquely most-specific signature or fail at compilation as
ambiguous. Module authors should add parameter annotations, use distinct names,
or make the call's argument type explicit to remove ambiguity. No source
syntax or portable-bytecode migration is introduced.
