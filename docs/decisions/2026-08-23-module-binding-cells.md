# Use shared cells for module bindings

## Context

Modules are cached before their top-level statements run so cyclic imports can
resolve. An importer must therefore be able to refer to a binding before its
defining module has initialized it, and later observe that same binding after
initialization.

## Decision

Statically knowable top-level module bindings are predeclared as shared cells
containing an internal uninitialized marker. Export maps expose cell-backed
binding values. Reading an uninitialized binding is a checked runtime error;
defining the binding replaces the marker in the existing cell.

## Consequences

- A module instance can enter the loader cache before top-level evaluation.
- Cyclic imports resolve without recursive initialization.
- The same cell representation provides the foundation for live imported
  bindings. Operations consuming a binding value must resolve it before use.
- Private VM bytecode remains unchanged as a portability contract.

## Migration

None.
