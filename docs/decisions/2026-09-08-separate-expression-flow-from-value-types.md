# Separate expression flow from value types

## Context

Phase 5 needs guard clauses and terminating match alternatives to preserve
facts into following expressions. A terminating `return` still has a payload
whose type participates in function result inference, so `never` alone cannot
represent both concerns reliably.

## Decision

Semantic expression checking records a value type and a continuation outcome
separately. The outcome distinguishes expressions that fall through from ones
that terminate the current control flow. `return`, `throw`, and `recur(...)`
are terminating expressions.

## Consequences

Branch and match environment merging can use continuation outcomes without
discarding return payload types. This remains deliberately small flow analysis;
it does not introduce a general control-flow graph or predicate prover.

## Migration

None.
