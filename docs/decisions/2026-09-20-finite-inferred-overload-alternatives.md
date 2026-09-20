# Infer finite overload alternatives from operator bodies

## Context

[Infer unannotated parameters from function bodies](2026-09-20-body-derived-parameter-inference.md)
establishes a singleton inferred signature when a body unambiguously requires
`num`. That model cannot describe overloaded operators. A body containing
`value + other` accepts correlated pairs such as `(num, num)` and `(bytes,
bytes)`, but independent union parameter types would incorrectly accept a pair
such as `(num, bytes)`.

Caller-derived inference is not an answer: callers must not change a
function's meaning. The body needs a way to publish the finite set of operand
relationships it establishes.

## Decision

Recognized overloaded operators MAY derive a finite set of inferred overload
alternatives from their declaring function body. An alternative is a symbolic
call scheme: an ordered parameter tuple, a result type, and generic variables
shared by those positions. The body creates the alternatives before any caller
is checked; callers only select or validate an existing alternative.

For `+`, the initial alternatives are:

```text
(num, num)                -> num
(str, T)                  -> str
(list<T>, list<U>)        -> list<T | U>
(bytes, bytes)            -> bytes
(map<K,V>, map<K2,V2>)    -> map<K | K2, V | V2>
```

The variables in a scheme are symbolic and scoped to that alternative. They
are instantiated for a call; they are not replaced by independent parameter
unions. Constraints from other expressions in the same body intersect the
alternative set. For example, `fn(left, right) { left + right; left / 10 }`
retains only `(num, num) -> num`.

A callable with more than one surviving alternative retains one source body
and one runtime implementation. Its alternatives are private semantic metadata
for statically known direct calls and exported module signatures. They are not
synthetic source declarations, and they do not change runtime argument binding.
A call with known argument types must select one applicable alternative under
the ordinary overload-specificity rules; no applicable alternative is a source
error. A dynamic call or a call through a structural function value that does
not retain this metadata remains dynamic.

The shared body continues to lower overloaded operations generically. This
decision adds static checking and result facts, not speculative specialization
or function-entry runtime type validation.

## Consequences

- Inferred `+` relationships remain sound: `(num, bytes)` is not admitted by
  merely widening both parameters to unions.
- Known callers gain precise result types and diagnostics without participating
  in inference.
- Imports can preserve the alternatives through existing callable-signature
  metadata, while unknown and structural calls deliberately lose that precision.
- The semantic model needs symbolic scheme variables, alternative intersection,
  and candidate selection for one implementation body.
- Numeric lowering remains governed by runtime-safe facts and is not implicitly
  enabled merely because one direct caller selected a numeric alternative.

## Migration

Known calls that were previously dynamic may become source errors when no
body-published alternative accepts their argument types. Dynamic calls retain
the existing runtime behavior. No source syntax is added.
