# Parameter requirements combine to the narrower type

## Status

Accepted

## Context

An unannotated parameter takes its type from requirements in its body:
arithmetic, ordering against a literal, and the parameters of a known callee it
is passed to. Two requirements had to be equal types, or the function failed
to compile. A callee's unconstrained parameter is `any|nil`, so a number
parameter that was also passed on to such a helper failed with
`parameter has incompatible body requirements: num and any|nil`, although every
number is acceptable there. An agent writing a JSON library hit this 6 times in
one run and had to annotate the parameter. See the
[agent-facing direction](../planning/agent-facing-direction.md).

## Decision

All of a parameter's requirements must hold, so its type is their
intersection. When one requirement's type is assignable to the other's, that
narrower type is the intersection and becomes the parameter's type. When
neither is assignable to the other, as with `num` and `str`, the function is
still a static error.

## Consequences

### Positive

- Passing a parameter on to a more permissive callee no longer conflicts with
  the body's own requirements.

### Negative

None known.

### Neutral

Requirements of which neither contains the other, such as `num|str` and
`num|nil`, still conflict rather than intersecting to `num`.

## Migration

None. Programs that compiled before infer the same types.
