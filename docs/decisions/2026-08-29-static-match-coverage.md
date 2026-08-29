# Diagnose closed type-constrained match coverage

## Context

Whole-case type constraints let Slug describe a match over a known union, but
the checker still accepts a case that can never match and cannot point out a
missing member of a closed union. Runtime matching must remain first-match and
must continue to yield `nil` when no case matches.

## Decision

Under `-type-check`, analyze coverage only when the subject has a closed union
of direct runtime categories or exact `struct<Name>` identities. `any`,
`unknown`, collection types, parameterized runtime values, and generic
`struct` identity keep the analysis conservative.

An unguarded irrefutable pattern—`_`, a binding, or `name @` around one—covers
its complete type constraint. An unguarded unconstrained irrefutable pattern
covers every remaining subject type. List, map, literal, pinned, and other
structural patterns do not establish coverage. A guard is always treated as
possibly false.

The checker reports a type-constrained case that is disjoint from the
remaining closed subject type, an unguarded case made unreachable by earlier
coverage, and a match that leaves a named closed member uncovered. These are
static diagnostics only; they do not add a runtime exhaustiveness failure.

## Consequences

- Typed union matches receive useful feedback without changing dynamic match
  semantics.
- Guards and structural patterns stay safe and conservative rather than being
  mistaken for total coverage.
- Exact nominal struct unions can be checked, while broad `struct` values and
  dynamic values remain outside the diagnostic domain.

## Migration

Programs run with `-type-check` may need to remove impossible cases or handle
every member of a closed subject union. Programs without that flag are
unchanged.
