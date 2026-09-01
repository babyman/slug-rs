# Match and Destructuring

`match` evaluates its optional subject once and tests cases from top to bottom.
The first matching case whose optional `if` guard is true supplies the result.
If no case matches, the result is `nil`.

```slug
match value {
  0, 1 => "small"
  n if n > 1 => "large"
  _ => "other"
}
```

When the subject is a map literal, parenthesize it to distinguish it from the
case block:

```slug
match ({name: "Slug"}) { {name} => name }
```

A match in a pipeline omits its subject because the pipeline supplies it:

```slug
value /> match { [head, ...] => head; _ => nil }
```

## Patterns

The target language pattern forms are:

- literals, including `nil`, numbers, strings, and booleans;
- `_` for a wildcard;
- an identifier to bind the matched value;
- `^name` to compare with an existing enclosing binding;
- `name @ pattern` to bind a value as well as require a nested pattern;
- list patterns, with a final spread pattern such as `[head, ...tail]`;
- map patterns, with a final spread entry such as `{name, ...rest}`;
- a top-level declaration map selector, `{*}`, which binds every string-keyed
  map entry into the module scope;
- exact map patterns, delimited by `{|` and `|}`, which reject extra keys and
do not allow a spread entry; and
- a type constraint attached to a complete match-case pattern, such as
  `user @ {name}: struct<User>`.

A map entry without `:` uses a bare key name and binds it to a same-named
identifier. For example, `{name}` requires the `"name"` key and binds its value
to `name`. A quoted static string is also a string key, so `{"status": 200}`
matches that exact key-value pair. A bracketed map-pattern key evaluates its
expression once before its containing pattern is tested. For a case with
alternatives, all computed key expressions are evaluated in pattern traversal
order before any alternative is tested. Each expression uses the enclosing
lexical scope, before any bindings from that pattern exist, and its result must
be a valid map key. Unlike a bare identifier key, quoted and bracketed keys
must be followed by `:` and an explicit value pattern.

```slug
match user {
  {name, age: years, ...rest} => name
  {"status": 200} => "ready"
  {[field]: value} => value
  {|name: "Slug"|} => "exact"
  _ => "other"
}
```

A list or map spread is final. An unnamed `...` discards the remainder and a
named form binds it. Comma-separated alternatives in one case are permitted
only when none of the alternatives introduces a binding.

`{*}` is a declaration-only form and is valid only at module top level. Its
right-hand side must be a map whose keys are strings. Each entry defines a
top-level binding with its key as the name and its value as the binding value.
It is intended for selecting a module's exported values, for example
`val {*} = import("slug.std")`; it is not a rest pattern and cannot be mixed
with ordinary map-pattern entries.

### Type constraints

A case pattern may end in `: Type`. The constraint applies to the whole
pattern: the subject must satisfy both the type constraint and the structural
pattern before the optional guard is evaluated. A failed constraint is an
ordinary failed match, not a runtime type error.

```slug
match value {
  user @ {age: 43, name}: struct<User> => name
  {|k1, k2|}: map<str, str> => "two strings"
  b: bool => "$b is bool"
  _: struct => "another struct"
  _ => "other"
}
```

Type constraints are permitted only on whole case patterns. They are not
declaration annotations and cannot appear within list, map, or struct fields.
Consequently, `val value: str = "Slug"` remains a declaration annotation and
`{name: pattern}` remains a map-pattern entry.

The runtime-checkable annotations are the direct value categories `nil`,
`bool`, `num`, `str`, `bytes`, `list`, `map`, `fn`, `task`, `chan`, `schema`,
and `struct`; `struct<Name>` schema identity; unions of runtime-checkable types;
and recursively checked `list<T>` and `map<K, V>` forms. `any` matches every
non-nil value, and `any|nil` matches every value. A collection constraint
checks every element or entry, including entries captured by a rest pattern.

Function signatures, task and channel payload annotations, tuple types, and
generic parameters are not runtime-checkable. Using one as a case constraint
is a source error. A `struct<Name>` constraint resolves `Name` as a schema
binding; an unknown name is a source error, while a resolved value that is not
a schema follows the checked runtime type-error path.

When optional type checking is enabled, a successful constraint narrows the
case bindings. For example, `b` has type `bool` in `b: bool`, and `name` has
type `str` in `{name}: map<str, str>`.

For a known `struct<S>` constraint, a named map-pattern field uses the field
type declared by `S`. The same field precision applies to declaration
destructuring of a known `struct<S>`.

The current Rust subset implements type constraints, including recursive list
and map checks, schema identity, and case-local narrowing under `-type-check`.

`var` and `val` accept these patterns on their left side. A non-matching
destructuring declaration follows the language error path. See
[slug.ebnf](slug.ebnf) for the grammar and the Language Specification for
function match bodies.
