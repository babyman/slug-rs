# Local functions refer to themselves by name

## Status

Accepted

## Context

A module-level function can call itself by name, because module names are
resolved when they are used. A function declared inside another function
could not: the bytecode compiler bound its local only after compiling the
function, so the body's reference fell through to a module lookup and failed
when called with `unknown name`. The semantic checker already declared the
name before checking the body, so the two disagreed, and the program compiled
cleanly. An agent writing a JSON library wrote
`val go = fn(out) { ... go(out :+ c) }` inside its parser and hit this at
runtime. See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

`val name = fn ...` and `var name = fn ...` inside a function bind `name`
before the function is compiled, so its body refers to the new binding.
Captures are live, so the binding holds the function by the time the body
runs. A later overload of an existing local name keeps its existing behavior:
it refers to the combined overload set.

## Consequences

### Positive

- Local recursive helpers work as they do at module level, without `recur`.
- The compiler agrees with the checker about which binding the name means.

### Negative

- A local function that wraps an outer function under the same name, as in
  `val log = fn(x) { log("> " + x) }`, now calls itself rather than the outer
  `log`, and so recurses without end. Such a wrapper needs a different name.

### Neutral

- Inside its own body, the name shadows an outer binding of the same name.
- Mutual recursion between two local functions still needs the second one
  declared before the first calls it.

## Migration

A local function whose body called an outer function of the same name must
rename one of them. The checker already treated that call as recursive, so it
typed the program as the compiler now runs it.
