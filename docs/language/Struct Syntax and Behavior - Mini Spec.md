# Structs

This supplement defines schema values, basic struct construction, field access,
and equality. Struct copy expressions, field type annotations, and struct
patterns are specified by the target grammar but remain separate implementation
stages.

## Schemas and defaults

A struct expression creates a new schema identity every time it is evaluated.
Fields are ordered by their declaration order and names must be unique. A field
without a default is required.

    val User = struct {
      name,
      active = true,
    }

Default expressions evaluate once, in source order, when the schema expression
is evaluated. They use the schema expression's lexical environment. Constructed
values reuse the resulting default values; construction does not reevaluate
default expressions.

The current Rust subset accepts untyped fields with optional defaults. Field
type annotations remain unsupported.

## Construction

Applying a schema to a brace-delimited field list constructs a value:

    val user = User {name: "Slug"}

The schema expression is evaluated first, followed by provided field expressions
in source order. Construction fails through the checked runtime type-error path
when the target is not a schema, a field is unknown, a field is provided more
than once, or a required field is omitted. Omitted fields with defaults receive
their schema's stored default values.

An empty construction used directly as a match subject is parenthesized to
distinguish its empty field list from the match case block:

    match (Marker {}) { _ => true }

## Access and equality

Dot access and string-key bracket access read a struct field. Accessing an
unknown field or using a non-string struct index follows the checked runtime
type-error path.

Each schema compares equal only to itself. Two struct values compare equal when
they have the same schema identity and their field values compare equal in
schema order. Values created from distinct schema evaluations are unequal even
when their field names and values are otherwise identical.

## Copying

`value copy { field: replacement }` creates a new struct value with the same
schema identity as `value`. It evaluates `value` first and replacement
expressions left to right. Each named field is replaced; fields not named in
the copy retain their original values. Copying a non-struct, naming an unknown
field, or naming a field more than once is a checked runtime type error.
