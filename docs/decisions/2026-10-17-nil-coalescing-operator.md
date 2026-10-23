# Nil-coalescing operator

## Status

Accepted

## Context

Several standard functions return `T|nil`, among them `readLine`, and so does
a map lookup of a key that may be absent. Turning such a value into a `T`
took a match, `match fs.readLine(file) { nil => "" ; line => line }`, or an
`if`. The most frequent type error across 90 agent evaluation runs was
`expected str, got str|nil`, 16 times, mostly a `readLine` result passed where
a `str` was required. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

`a ?? b` is `a` unless `a` is `nil`, in which case it evaluates and is `b`.

- Only `nil` selects `b`; `false`, `0`, and `""` do not, which is what
  distinguishes `??` from `||`.
- `b` is evaluated only when `a` is `nil`.
- Its type is `a`'s type without `nil`, joined with `b`'s: `str|nil ?? ""` is
  `str`. When `a` cannot be `nil`, the type is `a`'s.
- It binds more tightly than comparison and less than bitwise and arithmetic
  operators, as in Swift and Kotlin, so `count ?? 0 > 3` compares
  `count ?? 0`. It groups to the right, so `a ?? b ?? c` tries each in turn.
- `??` is a new token. Three question marks remain the `???` placeholder. A
  line beginning with `??` continues the previous expression, while one
  beginning with `???` does not.

It lowers to existing bytecode: duplicate, compare with `nil`, and jump.

## Consequences

### Positive

- A defaulted optional value is one expression with a precise type.
- It composes with nil-initialized fields: `user.nickname ?? user.name`.

### Negative

- One more operator and precedence level to learn.

### Neutral

- Safe navigation, `a?.b`, is not included. It can be added if evaluations
  show a need.

## Migration

None. `??` was a syntax error before.
