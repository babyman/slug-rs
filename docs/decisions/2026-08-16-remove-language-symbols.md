# Remove symbols from the Slug value model

## Context

Slug defines symbols as string-like values such as `:ok` and uses them for
bare map keys, dot access, map patterns, module exports, and configuration
results. Strings can represent every one of these names. Keeping both values
requires users and implementations to distinguish two textual key types, while
the specified dot-access fallback deliberately blurs that distinction.

The Rust runtime does not intern symbols or give them identity semantics, so
symbols provide no performance or representation benefit. Slug is also still
before its portable compiled-module implementation, making this the least
costly point to simplify the value model.

## Decision

Symbols are removed from Slug's syntax, value model, type categories, and map
key types. A bare identifier key in a map literal or pattern denotes a string
key. Dot access uses that same string key, so these expressions are equivalent:

```slug
val user = {name: "Slug"}
user.name
user["name"]
```

Module export maps, configuration maps, and other name-keyed language APIs use
strings. Closed sets of named alternatives should use strings until Slug gains
a dedicated enum or tagged-value feature.

Compiler-internal identifiers may be interned or represented as numeric IDs,
but those representations are not observable language values.

## Consequences

- Map literals, patterns, indexing, dot access, serialization, foreign
  interfaces, and type checking share one textual key type.
- Dot access no longer needs symbol-first lookup with a string fallback.
- Slug loses the `sym` type and the `:name` and `:"name"` literal forms.
- Future enum-like values require an explicit language feature rather than an
  informal symbol convention.
- Runtime, source frontend, tests, and language documentation must not expose a
  symbol value or describe symbol-keyed maps.

## Migration

Replace symbol literals with strings, such as `:ok` with `"ok"`. Replace
symbol-key indexing such as `map[:name]` with `map["name"]`; bare map keys and
dot access keep their spelling but now address string keys. Replace `sym` type
annotations with `str`, including `map<sym, V>` with `map<str, V>`.
Rust embedders must replace `Value::Symbol` and `Value::symbol` with
`Value::Str` or `Value::string`.
