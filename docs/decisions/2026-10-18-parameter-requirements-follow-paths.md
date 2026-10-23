# Parameter requirements follow paths

## Status

Accepted. Amends
[Parameter requirements combine to the narrower type](2026-10-17-parameter-requirements-combine.md):
the requirements that combine are those on one path.

## Context

An unannotated parameter took its type from every requirement anywhere in its
body, all of which had to hold. A requirement in one branch therefore applied
to every other branch, including one a runtime check had chosen for other
types. The Tiny interpreter's `arithmetic(node, a, b)` returns `a + b` early
for strings and lists, and divides in a later branch:

```slug
if (op == "+" && typeOf(a) == typeOf(b) && ...) { return a + b }
match op { "/" => int(a / b) ... }
```

The division made `a` a `num`, so a call with strings was rejected, and with
dynamic arguments the compiler emitted numeric-only addition for the early
`return`, which faulted at runtime with `expected numbers, got str and str`.

## Decision

A parameter's requirements are collected per path. Along one path they all
hold, combining as before. Where paths meet, after an `if` or `match`, the
right operand of `&&`, `||`, or `??`, or at the end of the body for the paths
that left through `return`, a parameter keeps a requirement only when every
path that still runs has the same one. A path that throws does not finish and
adds nothing.

On the path where a parameter equals a literal, such as the `then` branch of
`if (n == 0)` or the skipped right operand of `c != "" && ...`, the parameter
has the literal's type, so `if (n == 0) { base } else { recur(n - 1) }` still
infers `num`. A nil test's `nil` path takes the other path's requirement, which
the nil partition then widens to `T|nil`, as before. A parameter default runs
whenever its argument is omitted, so its requirements hold on every path.

## Consequences

### Positive

- A runtime type check that sends other types down its own path is respected:
  `arithmetic("+", "a", "b")` compiles and runs, and no path is compiled with
  an operation specialized for another path's types.

### Negative

- A function with a path that does not use a parameter leaves it dynamic.
  Measured on 209 files, 52 of 999 functions lose an inferred parameter type,
  and none gain one. Most are recursive helpers whose base case returns a
  parameter untouched, such as `gcd(a, b)` returning `a` when `b == 0`; a wrong
  argument to them is now found when it runs rather than when compiling.

### Neutral

- The same files compile or fail as before, with the same messages.
- In an interleaved benchmark comparison, the two benchmarks whose inference
  changed ran in 1.02 and 1.00 times their previous time, within noise.
- A `match` case with literal patterns does not give its subject the
  literal's type: a `match` over string literals with no `_` case would then
  be reported as non-exhaustive.

## Migration

Annotate a parameter that should keep a static type, such as `gcd(a:num, b)`.
