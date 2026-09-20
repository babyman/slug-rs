# Infer unannotated parameters from function bodies

## Context

An unannotated parameter currently has the universal `any|nil` type. That is a
safe fallback, but it discards requirements that the function body proves. For
example, the division in `fn(value) { value / 10 }` cannot complete unless
`value` is `num`. Retaining `any|nil` prevents static call diagnostics and
numeric bytecode lowering for this common shape.

Deriving a parameter type from callers would make a function's meaning depend
on use order, reachability, imports, and whether a function value escaped.
That would be particularly brittle for recursive functions and overloads.

## Decision

Unannotated parameters derive their type from constraints in their declaring
function body. Call arguments validate against the completed signature; they
MUST NOT contribute constraints used to infer it.

The initial inference domain is deliberately narrow. Operators whose only
successful operand family is numeric—division, modulo, unary negation, and
ordinary ordering comparisons—can constrain an unannotated operand to `num`.
An explicit parameter annotation remains authoritative. A parameter with no
unique body-derived type retains `any|nil`.

Body-derived parameter types are public callable-signature facts. They
participate in direct-call checking, overload applicability and identity,
structural function types, and exported module metadata exactly as an explicit
annotation would. Thus these declarations have the same input signature:

```slug
val inferred = fn(value) { value / 10 }
val explicit = fn(value:num) { value / 10 }
```

Overloaded operators do not establish a parameter type in the initial domain.
For example, `value + 10` remains ambiguous between numeric addition and
string concatenation. Future constraint work may add disjunctive relationships
for `+`, `-`, and `*`, but it must not weaken this caller-independence rule.

## Consequences

- `fn(value) { value / 10 }` has an inferred `num` input and a known call with
  a string is a source error.
- A dynamic call remains valid source and retains the ordinary checked runtime
  fault if it supplies a non-number; inference adds neither coercion nor a
  general function-entry type guard.
- Recursive bodies need a stable local constraint-solving pass before their
  final callable metadata is installed.
- Inferred and explicit equivalent signatures collide as duplicate overloads.
- Imported modules must export inferred signatures; importing code must not
  re-infer them from its own calls.

## Migration

Existing statically known calls that pass an incompatible value to a
body-inferred parameter become source errors. Add an explicit broad annotation
such as `any|nil`, or remove the constraining operation, when dynamic behavior
is intentional. Calls whose argument type is dynamic retain runtime behavior.
