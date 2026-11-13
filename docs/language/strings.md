# Strings

Slug has raw and interpolated string literals in single-line and
triple-quoted forms.

```slug
val raw = 'C:\\Program Files\\Slug'
val text = "line one\\nline two"
val value = "Hello, $name"
val total = "Total: ${sum(prices)} for ${order.count} items"
val template = '''literal $name'''
val multi = """
first
second
"""
```

Single-quoted strings are raw: escapes and interpolation are not
interpreted. Double-quoted strings support interpolation and these escapes:
`\n`, `\r`, `\t`, `\b` (backspace), `\f` (form feed), `\\`, `\"`, `\$`, and one
to three octal digits. An unknown
escape remains a backslash followed by its character.

An interpolation is either `$` followed by an identifier, which resolves that
identifier in the current lexical environment, or `${expression}`, which
evaluates any expression there: a call, member access, index, arithmetic, an
`if` or `match`, or another string, interpolated too. Braces and strings
nest inside the expression, so `"${m["key"]}"` and `"${{a: 1}}"` work.
Interpolations evaluate once each, left to right, and each value becomes text
as `println` shows it: a string as it is, anything else in its display form.
`\$` writes a literal `$`, so `"\${x}"` is the text `${x}`; a `$` followed by
neither an identifier nor `{` is a literal `$`. `$name.field` interpolates
`name` followed by the text `.field`; write `${name.field}` for the member.
An empty `${}` or an unterminated `${` is a syntax error.

Triple single quotes create a raw multiline string. Triple double quotes create
an interpolated multiline string. When a triple-quoted opening delimiter is
immediately followed by a newline, that leading newline is omitted. A newline
immediately before the closing delimiter is also omitted. Other content,
including indentation, is preserved.

Unterminated string literals are lexical errors. The language does not support
Handlebars-style `#if`, `#each`, or `#with` blocks in strings.

## Comparison

`==` and `!=` compare strings by content. `<`, `<=`, `>`, and `>=` compare two
strings lexicographically by their UTF-8 bytes, which matches Unicode scalar
value order. Comparison is case-sensitive and does not depend on locale, so
every uppercase ASCII letter orders before every lowercase one.

```slug
"apple" < "banana"   // true
"Z" < "a"            // true
"ab" < "abc"         // true: a prefix orders first
```

Ordering a string against a number is a type error.

## Indexing and slicing

Strings support integer indexing and list-style slices. Positions count Unicode
scalar values rather than UTF-8 bytes, so indexing returns a one-character
string and cannot split an encoded character. Negative indices and slice bounds
count from the end; omitted slice bounds and steps follow list slicing rules.

```slug
"Slug"[3]      // "g"
"aébc"[1]      // "é"
"aébc"[1:-1]   // "éb"
```
