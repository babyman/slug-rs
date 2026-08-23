# Preserve schema identity when copying structs

## Context

The source grammar specifies `value copy { field: replacement }`, but the
initial struct foundation deliberately deferred its observable replacement
semantics.

## Decision

A copy evaluates its original value before replacement expressions, evaluates
replacements left to right, and creates a new value with the original schema
identity. Unnamed fields retain their original values. A non-struct original,
unknown field, or duplicate replacement field is a checked runtime type error.

## Consequences

Copied values remain comparable with their originals under the existing
schema-identity equality rule. The VM uses private copy metadata; no bytecode
representation becomes a compatibility promise.

## Migration

None.
