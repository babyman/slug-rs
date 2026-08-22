# Value Pinning in `match` Patterns

An identifier pattern binds a new name. Prefixing it with `^` instead compares
the candidate value with an existing lexical binding.

```slug
val expected = "ok"
match result {
  ^expected => "matched"
  _ => "other"
}
```

`^name` is an atomic pattern. `name` must resolve in an enclosing environment;
the pattern does not bind it. Equality failure makes the current case fail and
continues matching with the next case, without evaluating that case's guard.
Pinned values are read once when their case is attempted, before that case's
alternatives are tested. Reading a declared top-level binding before it has
initialized follows the normal runtime error path.

Pinning is valid wherever a match pattern is accepted, including list and map
patterns:

```slug
match response {
  {status: ^expected, body} => body
  _ => nil
}
```

A pinned identifier is non-binding, so it is valid in a comma-separated pattern
alternative. It is distinct from `^` as the bitwise-XOR operator, which occurs
only in expressions rather than in a pattern position.

The Rust VM passes pinned values through indexed dynamic pattern operands; see
[the dynamic pattern operand decision](../decisions/2026-08-22-dynamic-pattern-operands.md).
