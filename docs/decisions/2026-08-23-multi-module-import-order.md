# Preserve first-loaded bindings in multi-module imports

## Context

`import` accepts several module names so a program can assemble a namespace
from standard, test, and local-default modules. Name collisions need a stable
result that does not depend on loader timing.

## Decision

`import(name, ...)` loads modules in argument order. It merges their exported
bindings into one map. For a duplicate non-function name, the first loaded
binding remains and the implementation emits a warning. For duplicate callable
names with the same signature, the first loaded callable remains and the
implementation emits a warning. Callables with distinct signatures combine
into one overload set.

## Consequences

- Import precedence is visible from source order.
- Module caching cannot change which binding wins.
- Import implementations must retain callable signatures while merging exports.

## Migration

None.
