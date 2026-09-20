# Propagate uniquely selected inferred alternatives

## Context

Known direct calls can contribute an already-published singleton input and
result signature to their enclosing function body. A callable with inferred
overload alternatives instead describes several correlated schemes. Blindly
copying all of those schemes through a wrapper would be alternative-set
forwarding and would require composition rules that do not yet exist.

## Decision

An ordinary, direct call to one known, non-generic callable MAY contribute the
input constraints and result fact from an inferred alternative only when
independent body facts select exactly one alternative. Facts already derived
from the caller's own expressions, such as `left / 10`, may select an
alternative. The selected alternative then constrains the arguments passed to
that call and its instantiated result participates in ordinary body-result
checking.

The call's alternatives must not contribute facts while determining which
alternative applies. If the callee is dynamic or structural, an argument is a
spread, binding is incomplete, or zero or multiple alternatives apply, the
call contributes no body-derived constraint. In particular,
`fn(left, right) { combine(left, right) }` does not forward `combine`'s entire
alternative set.

## Consequences

- A wrapper can retain a selected relationship without introducing function
  specialization, runtime entry contracts, or caller-derived inference.
- A numeric body fact can make a call to `fn(left, right) { left + right }`
  constrain both wrapper inputs to `num` and infer a `num` result.
- Unannotated wrappers around overloaded callables remain dynamic until a
  later composition decision defines alternative-set forwarding.

## Migration

Calls to wrappers whose bodies independently select an inferred alternative
may now reject statically known incompatible arguments. Dynamic calls retain
their checked runtime behavior. No source syntax changes.
