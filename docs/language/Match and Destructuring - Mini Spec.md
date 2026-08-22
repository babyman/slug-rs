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
- exact map patterns, delimited by `{|` and `|}`, which reject extra keys and
do not allow a spread entry; and
- struct patterns such as `User {name}`.

A map entry without `:` uses the key name and binds it to a same-named
identifier. For example, `{name}` requires the `"name"` key and binds its value
to `name`. A map-pattern key may be bracketed to evaluate a key expression.

```slug
match user {
  {name, age: years, ...rest} => name
  {|name: "Slug"|} => "exact"
  _ => "other"
}
```

A list or map spread is final. An unnamed `...` discards the remainder and a
named form binds it. Comma-separated alternatives in one case are permitted
only when none of the alternatives introduces a binding.

The current Rust subset implements literals, `_`, identifier bindings, list
patterns with an optional named or anonymous final spread, and string-keyed map
patterns with an optional named or anonymous final spread. It also implements
exact map patterns. Pinning, `@` patterns, alternatives, computed map keys,
and struct patterns remain specified but unsupported; see the generated
language support matrix for the implemented subset.

`var` and `val` accept these patterns on their left side. A non-matching
destructuring declaration follows the language error path. See
[slug.ebnf](slug.ebnf) for the grammar and the Language Specification for
function match bodies.
