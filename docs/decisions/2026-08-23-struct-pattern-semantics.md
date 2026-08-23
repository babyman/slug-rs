# Match structs by schema identity

## Context

The grammar specifies `Schema {field}` patterns but did not define whether
matching is structural, whether fields are partial, or how a non-schema pattern
designator behaves.

## Decision

Struct patterns match only values created by the exact schema identity named by
their designator. Named fields are partial requirements: each must exist and
match, but omitted fields are ignored. The designator must evaluate to a struct
schema or evaluation reports a checked runtime type error. Duplicate field
names are invalid source.

## Consequences

Patterns remain aligned with struct equality and copies, both of which preserve
schema identity. Struct patterns cannot accidentally match lookalike values
from a different schema.

## Migration

None.
