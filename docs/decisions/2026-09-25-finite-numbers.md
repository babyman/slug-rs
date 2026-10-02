# Finite Slug Numbers

## Context

IEEE-754 NaN and infinities make equality, ordering, map keys, serialization,
and native boundaries carry exceptional rules that do not help ordinary Slug
programs.

## Decision

Every Slug `num` is finite. Any source literal, arithmetic result, private
bytecode constant, or native result containing NaN or either infinity is
rejected with a checked error. `int(value)` therefore only needs to preserve
integers or truncate a finite float toward zero, subject to the `i64` range.

## Consequences

Floating-point rounding remains ordinary binary64 behavior, but non-finite
values never participate in Slug comparisons, maps, or calls. Hosts must
validate recursive native results before exposing them to Slug.

## Migration

Programs and private bytecode that depended on NaN or infinity now fail with a
checked error. Native providers must return finite floating-point values.
