# Local functions are visible throughout their block

## Status

Accepted

## Context

Module-level names resolve when they are used, so module-level functions may
call one another in any order. A function declared inside another function
could call only functions declared before it: a reference to a later one
compiled to a module lookup and failed at runtime with `unknown name`, with
no compile-time error. A recursive-descent parser, whose `primary` calls
`expression` which calls `primary`, cannot be written that way without
forward declarations such as `var expression = nil`. This came up while
writing the reference for the Tiny interpreter evaluation project. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Inside a function, a block's `val name = fn ...` and `var name = fn ...`
declarations are visible throughout that block. The compiler declares their
slots when the block begins; captures are live, so a function that calls a
later one finds it once its declaration has run.

- Using such a function directly, rather than from inside a nested function,
  before its declaration in the same function is a static error, since it
  would find no function yet.
- Other declarations are not hoisted: a nested function could read them
  before they are assigned. Using a name the block declares later, and that no
  enclosing scope binds, is a static error at the use.
- The checker treats a block's functions as declared from its start.

## Consequences

### Positive

- Local helpers can be mutually recursive in any order.
- Using a local name too early is a static error rather than a runtime
  `unknown name`.

### Negative

- A block that calls an outer function and later declares a local function of
  the same name now reports a use before declaration. The corpus of 201 Slug
  files has none.

### Neutral

- A value built by a call, such as a parser combinator's result, still needs
  to be declared before functions that use it, for example with
  `var expression = nil`.

## Migration

Rename a local function that shadows an outer one called earlier in the same
block.
