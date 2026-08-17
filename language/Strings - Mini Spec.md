# Strings

Slug has raw and interpolated string literals in single-line and triple-quoted
forms.

```slug
val raw = 'C:\\Program Files\\Slug'
val text = "line one\\nline two"
val value = "Hello, {{name}}"
val template = '''literal {{name}}'''
val multi = """
first
second
"""
```

Single-quoted strings are raw: escapes and `{{...}}` interpolation are not
interpreted. Double-quoted strings support interpolation and these escapes:
`\n`, `\r`, `\t`, `\\`, `\"`, `\{`, and one to three octal digits. An unknown
escape remains a backslash followed by its character. Interpolation evaluates
a Slug expression in the current environment.

Triple single quotes create a raw multiline string. Triple double quotes create
an interpolated multiline string. When a triple-quoted opening delimiter is
immediately followed by a newline, that leading newline is omitted. A newline
immediately before the closing delimiter is also omitted. Other content,
including indentation, is preserved.

Unterminated string literals are lexical errors. The language does not support
Handlebars-style `#if`, `#each`, or `#with` blocks in strings.
