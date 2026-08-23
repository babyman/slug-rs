# Use an export declaration keyword

## Context

Module visibility changes how another module can resolve a top-level binding.
Representing that behavior as `@export` makes a language rule look like
ordinary metadata, even though tags otherwise do not change evaluation
semantics. It also forces tools to recognize one special tag among an open set
of metadata names.

## Decision

Use `export` as a reserved declaration modifier for top-level `val`, `var`, and
`foreign` declarations:

```slug
export val increment = fn(n) { n + 1 }
export foreign trim = fn(value)
```

Tags may precede an exported declaration. `@export` remains valid ordinary tag
syntax, but has no export semantics. `export` is invalid on a declaration in a
nested scope.

## Consequences

- Module visibility is explicit in the grammar and no longer appears to be
  optional metadata.
- Parsers, formatters, semantic analysis, documentation tools, and module
  loaders must recognize the declaration modifier.
- General tag processing no longer needs special export behavior.
- The keyword cannot be used as an identifier or tag name.

## Migration

Replace `@export` followed by a declaration with `export` before that
declaration. For example, replace `@export val answer = 42` with
`export val answer = 42`.
