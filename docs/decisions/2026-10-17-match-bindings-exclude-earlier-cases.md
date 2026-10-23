# Match bindings exclude what earlier cases match

## Status

Accepted

## Context

A match case's binding or wildcard pattern without a type constraint was
typed as the whole subject type, even when earlier cases had already matched
part of it. The documented way to read a line,
`match fs.readLine(file) { nil => "" ; line => line }`, therefore typed `line`
as `str|nil`, and a function declaring a `str` result failed with
`expected str, got str|nil`. An agent writing a JSON library hit this twice in
one run and worked around it with `line: str => line ; _ => ""`. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

Cases are tried in order, so a value reaching a case is not one an earlier
case matched. The checker tracks the subject type that can still reach each
case. An unguarded case removes from it:

- everything, for a binding or wildcard pattern without a constraint;
- its constraint, for a binding or wildcard pattern with one;
- `nil`, for a `nil` literal pattern.

A binding or wildcard case without a constraint binds the remaining type, and
when the subject is a name it narrows that name inside the case. Guarded cases
remove nothing, since their guard may fail. Other patterns, such as lists,
maps, and other literals, also remove nothing, since they match only part of
a type.

## Consequences

### Positive

- The common `nil => ... ; value => value` form types `value` without `nil`.

### Negative

None known.

### Neutral

- Exhaustiveness and unreachable-case checking are unchanged.
- `true` and `false` cases together do not remove `bool`.

## Migration

None. Bindings only become narrower, and a narrower binding is accepted
wherever the wider one was.
