# A binding keeps a value's dynamic type

## Status

Accepted. Settles the question that
[A binding's type is set by its first value](2026-10-17-a-bindings-type-is-set-by-its-first-value.md)
left open, for unannotated bindings.

## Context

A value whose type the checker cannot infer, such as the result of a
recursive helper whose parameters stay dynamic, is dynamic where it is used
directly: it is checked when it runs. Binding it first widened it to the
static `any|nil`, so naming an intermediate value changed what code could do
with it. In the Tiny project an agent wrote

```slug
val block = fn() { need("{"); val b = sequence("}"); need("}"); b }
var other = []
other = if (...) { [statement()] } else { block() }
```

and got `expected list, got any|nil`, which `fn() { sequence("}") }` would not
have produced. It annotated two functions to continue.

## Decision

An unannotated `val` or `var` binding keeps its initializer's type as the
checker found it, including any part it could not infer, which stays dynamic.
This covers a whole value, as in `val b = sequence("}")`, and a nested one, as
in `val xs = [helper()]`. An annotated binding takes its annotation, and a
struct field's default is still widened, as before.

## Consequences

### Positive

- Naming a value never changes its type: `val b = f(); b` and `f()` may be used
  in the same places.

### Negative

- A dynamic value can reach an annotated binding through a named intermediate,
  as in `val b = helper(); val l:list = b`, which is checked neither when
  compiling nor when it runs. The same was already true of
  `val l:list = helper()`.

### Neutral

- Measured on 212 Slug files, 133 bind a value whose type is partly unknown,
  and no file's compile outcome changes. The references score as before, the
  bytecode of every benchmark is identical, and compiling the corpus took
  2.19 s against 2.26 s before.

## Migration

None. Every program that compiled before compiles and behaves the same.
