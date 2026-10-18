# A `nil`-started var takes its first assignment's type

## Status

Accepted. Amends
[A binding's type is set by its first value](2026-10-17-a-bindings-type-is-set-by-its-first-value.md)
for a `var` whose first value is `nil`.

## Context

A `var` whose first value is `nil` was declared `any|nil`. Code that reads it
where an assignment is in view gets the assigned type, but a nested function
reads it by its declared type, so state filled in by one closure and read by
another needed an annotation. Agents hit this in the Tiny project: a
tokenizer's one-token lookahead, `var ahead = nil`, failed with
`expected map<...>, got any`, and the fix taken was to widen the reader, not
to annotate the `var`.

Two rules were prototyped and measured:

- Its declared type is `nil` with the types of every assignment. Nothing
  fails that compiled before, but a mistaken assignment widens the type
  instead of being reported, and is never reported where the `var` is only
  printed, compared, or passed on as `any`.
- Its declared type is `nil` with the type of its first assignment, and later
  assignments must conform, as they do for `var count = 0`.

## Decision

A `var` whose first value is `nil`, with no annotation, is declared `nil` with
the type of the first assignment to it written in its scope, including one in
a nested function. A trial check of the scope finds that assignment. An
accumulator such as `count = count + 1`, whose assigned type depends on the
`var`'s own, is found by repeating the trial, at most three times, until it
reproduces the type it started from.

It stays `any|nil` when there is no assignment, when the first one's type is
unknown, or when the trial does not settle. A trial in which the `var` itself
caused an error also leaves it `any|nil`; an error in the first trial, in
which the `var` is dynamic, is not its own, so the assignments before it
still count.

A mismatch names where the type came from:

```text
expected num|nil, got str: `best` is `num|nil`, from its first value `nil`
and its first assignment at 2:8; to hold both, declare
`var best:num|str|nil = nil`
```

Struct fields whose default is `nil` remain `any|nil`.

## Consequences

### Positive

- State shared between closures, such as a lookahead token, checks without
  an annotation. The agent's original Tiny tokenizer compiles unchanged.
- A mistaken assignment is reported where it is written, as for any other
  `var`, and the message names the declaration that would hold both types.

### Negative

- A `var` that deliberately holds several types, such as a token's value that
  is a number or a name, needs its type declared. Code that compiled before
  can fail, at the first assignment of a second type.
- "First" is the first written, not the first run: an assignment in a closure
  written above one at top level sets the type, even if it runs later.
- A block with such a `var` is checked once more, or up to three more times
  for an accumulator. A trial runs no nested trial, so nesting does not
  multiply it. Real files measured 2 to 3 ms slower, about 15%; a file of 40
  functions each nesting several such `var`s took 1.8 times as long to check.

### Neutral

- Measured on 201 Slug files, including every workspace the evaluation agents
  wrote, the same 21 fail before and after, with the same messages. Four
  files declare such a `var`. One of them holds a number and a name in the
  same `var`, and still compiles only because its first assignment's type is
  unknown.

## Migration

Declare the type of a `var` that holds more than one type besides `nil`:
`var value:num|str|nil = nil`. The error names the declaration to write.
