# Optional List Slice Start

## Context

The historical slice grammar required a literal zero in a slice that began at
the first list item. That distinction was needed while labels were part of the
language, but labels are not present in this implementation.

## Decision

Allow the start expression in a list slice to be omitted. `list[:end]` is
equivalent to `list[0:end]`. Keep end and step optional as well, with defaults
documented in the language specification.

## Consequences

The parser must distinguish indexing from a slice whose first token is `:`.
The compiler records which slice operands are present in private bytecode, and
the VM supplies defaults while preserving checked type failures.

## Migration

Existing `list[0:end]` programs remain valid. New programs may use
`list[:end]`.
