# Named closed variants

## Status

Proposed; implementation deferred.

## Context

Slug can model a finite domain with separate schemas and a union such as
`struct<Ok>|struct<Err>`, but that repeats the domain at every annotation and
does not name the set of cases. The existing nominal-schema, field-metadata,
and match-coverage foundations can support a closed domain without replacing
open-ended schemas.

## Decision

When implemented, Slug will add named enum declarations with record-shaped
variants:

```slug
enum Result {
  Ok { value: str },
  Err { message: str },
}

val result: Result = Result.Ok { value: "done" }

match result {
  {value}: Result.Ok => value,
  {message}: Result.Err => message,
}
```

`Result` is a closed parent type. `Result.Ok` and `Result.Err` are exact,
nominal variant types; each is assignable to `Result`, but variants from a
different enum are not. Enum declarations use the existing named-field schema
rules for required fields, defaults, copies, field reads, and pattern binding.

The initial pattern form remains the existing whole-pattern type constraint,
`{value}: Result.Ok`. Variant-specific pattern syntax is deferred.

At runtime, each variant will use a distinct schema identity. The enum name is
a constructor namespace whose members are those variant schemas, so construction
and schema-identity matching reuse established runtime behavior. Static semantic
metadata records the parent enum identity and its fixed variants. `Result` is
therefore eligible for coverage diagnostics: an unhandled variant is
non-exhaustive, and a repeated covered variant is unreachable under
`-type-check`.

The initial feature excludes generic enums, positional payloads, variant
aliases, and additional pattern syntax. Ordinary schemas remain the mechanism
for open-ended records.

## Consequences

- Finite domains gain a single nominal annotation and useful exhaustive-match
  diagnostics.
- Existing schema field checking and stable nominal identity provide the
  payload and import foundations rather than requiring a new value layout.
- Annotation and constraint resolution must support qualified enum and variant
  names while retaining user-facing source spellings in diagnostics.
- Enum declarations, constructor namespaces, parent-type assignability, and
  coverage expansion are new language and semantic work; this record does not
  make them implemented behavior.

## Migration

None. Enums are not implemented or accepted source syntax yet. Existing
schema-and-union programs remain valid and can migrate voluntarily if and when
the feature is implemented.
