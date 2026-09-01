# Variadic Functions and Spread Syntax

A final function parameter may be prefixed with `...`. It receives a list of
all remaining positional arguments. A function has at most one variadic
parameter.

```slug
val collect = fn(first, ...rest) { [first, rest] }
collect(1, 2, 3) // [1, [2, 3]]
```

Calls accept a spread argument, `...expression`. Its expression must evaluate
to a list, whose elements are inserted at that point in left-to-right argument
evaluation order. Spreads may be mixed with ordinary arguments and may occur
more than once.

```slug
val values = [2, 3]
collect(1, ...values)
```

A non-list spread is a runtime error. Positional arguments bind before named
arguments. Once a named argument is used, every later argument must be named.
Named arguments use `=` and may set the variadic parameter only to a list.
Unknown, duplicate, and missing required arguments are language errors. Default
expressions provide values for omitted non-variadic parameters.

For an overloaded call that is otherwise tied, a non-variadic candidate wins
over a variadic candidate whose rest parameter would be empty. Declaration
order and the number of omitted defaults do not resolve overload ties.

The same `...` token also has distinct list-literal and match-pattern meanings:
`[...values]` spreads a list into a list literal, while `[head, ...tail]` is a
match pattern. Context determines which form applies.
