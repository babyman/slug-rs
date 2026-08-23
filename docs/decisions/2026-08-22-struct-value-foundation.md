# Give struct schemas identity and stored defaults

## Context

The target syntax includes schema expressions, struct construction, copy
expressions, and struct patterns, but the Rust runtime has no schema or struct
value. Treating structs as maps would lose schema identity and leave later
struct patterns unable to distinguish unrelated schemas with the same fields.

The language documents also did not decide when field defaults evaluate or how
schema and struct equality behave. Those outcomes are observable and must be
stable before bytecode and VM operations are introduced.

## Decision

Every evaluation of a struct expression creates a distinct schema identity. A
schema stores its fields in declaration order and stores the value of each
optional default expression. Default expressions evaluate once, in source
order, when the schema is created, using the surrounding lexical environment.

A struct value holds a reference to its schema plus one value per schema field.
Construction accepts named fields, rejects duplicate or unknown names, fills
omitted defaulted fields from the schema, and rejects omitted required fields.

Schema equality uses identity. Struct equality requires the same schema identity
and equal field values in schema order. Structs remain distinct runtime values;
they are not represented as maps.

Private bytecode carries ordered schema-field metadata and ordered construction
field names. The VM validates this metadata and all construction failures
remain checked Slug runtime errors.

## Consequences

- Later struct patterns can compare schema identity before matching fields.
- Field order and equality are deterministic without map-key lookup semantics.
- Defaults do not need deferred calls or hidden constructor frames.
- A default that reads mutable state observes it when the schema is created,
  not when each instance is constructed.
- Copy expressions, field annotations, and struct patterns can build on the
  same value representation without being included in this initial stage.

## Migration

No existing Slug source migration is required. The new bytecode and value
variants are private implementation details.

