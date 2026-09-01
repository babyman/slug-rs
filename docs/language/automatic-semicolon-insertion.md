# Automatic Semicolon Insertion

A program is a sequence of statements separated by `;` or by a newline that
terminates the preceding expression. Repeated separators and blank lines are
allowed. A closing `}` and end of file also finish the surrounding statement.

## Newline termination

A newline normally ends the current expression. It instead continues that
expression in either of these cases:

1. The token before the newline requires a right-hand side: assignment, a
binary operator, `/>`, `.`, `:`, or `=>`.
2. The first token after the newline is an infix continuation token: `+`, `-`,
`*`, `/`, `%`, equality, comparison, logical-and/or, bitwise-and/or, shift,
list append/prepend, `/>`, or `.`.

```slug
val total = first +
  second

val text = "a"
  + "b"
```

A newline before `(` or `[` always terminates. Calls and indexing must remain
on the same line as their receiver:

```slug
f(x)
items[0]
```

## Delimited forms

Newlines inside parentheses and brackets are whitespace. Map, struct, list,
and parameter forms use their own comma-delimited grammar. Match cases are
separated by newlines or semicolons, and a case body may start after a newline.
A newline before a pinned match case (`^name => ...`) separates cases even
though `^` is otherwise an infix continuation token.

Braces are structurally disambiguated between blocks and map literals. In an
expression position, write unambiguous map entries such as `{name: value}`,
`{"name": value}`, or `{[key]: value}`. A standalone brace form is a block. See
[slug.ebnf](slug.ebnf) for the precise productions.

These rules describe current parser behavior. When readability matters,
parenthesize a multiline expression rather than relying on a continuation rule.
