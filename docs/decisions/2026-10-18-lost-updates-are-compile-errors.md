# Lost updates are compile errors

## Status

Accepted

## Context

Slug's lists, maps, and strings are immutable, so code accumulates into a
`var` by reassigning it: `output = output :+ line`. When the value also calls
a function that assigns the same `var`, the order of evaluation decides the
result. In `output = output :+ render(evaluate(x))`, `output` is read first,
`evaluate` then appends nested `print` output to it, and the assignment
overwrites that with the earlier value plus the rendered result. The program
is valid and evaluates left to right as specified, but the inner change is
lost silently.

Agents writing the Tiny interpreter made this mistake in four of five runs:
two shipped it and failed hidden cases, and two found it with their own
tests. Python, the control arm, appends in place and cannot make it.

## Decision

An assignment `v = value` is a static `lost_update` error when `value` reads
`v` before a call returns, and that call can assign the same `v`. The check:

- resolves names lexically, with a block's declarations visible throughout
  it;
- follows only calls of functions declared with `val name = fn ...`, by
  name, and transitively through the functions they call by name;
- follows a call only through functions nested in the function that declares
  `v`. A path that leaves that function reaches `v` again only by calling it,
  which makes a new `v`, as in a recursive-descent parser whose `block`
  accumulates its own statements;
- ignores function literals inside `value`, which do not run with it.

The error is at the stale read and names the call, suggesting
`val value = call(...)` before the assignment.

## Consequences

### Positive

- The mistake is reported at compile time, at the read that loses the
  update. It flags the drafts in which agents made it, and the two finished
  libraries that shipped it, and no other file of 218.

### Negative

- Calls through values the check cannot follow, such as a function stored in
  a map or passed as an argument, are not checked.

### Neutral

- Compiling the corpus took 2.10 s with the check and 2.11 s without.

## Migration

Bind the call's result before reading the `var`:
`val value = render(evaluate(x)); output = output :+ value`.
