# Keep resource declarations in host module metadata

## Context

`resource Name` introduces a compile-time type name, while a native callback is
the only way to create a handle. Hosts nevertheless need to inspect a loaded
module's declared resource kinds when diagnosing or configuring native
integrations.

## Decision

Each resource declaration is retained as a `ModuleDeclaration` with its
`resource_type` name in host-facing module metadata. It has no corresponding
Slug runtime value, constructor, or export-map entry. A host obtains this
metadata from `ModuleInstance.metadata`; Slug programs continue to use the
name only in the compile-time type namespace.

## Consequences

Resource declarations are inspectable by hosts without making a resource type
forgeable, callable, or selectable as a value. The metadata remains part of
the private, in-process module representation and is not a portable bytecode
or source-level reflection contract.

## Migration

None.
