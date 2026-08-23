# Execute modules in isolated runtime instances

## Context

An imported module has its own globals, initialization order, cache identity,
and exported closures. Reusing the importing VM's global map would allow module
bindings to collide with caller bindings and would make an exported closure run
against the wrong bytecode program.

## Decision

Compile each resolved module once and execute it in a cached module instance
with its own globals and program. Imports expose only exported bindings from
that instance. Exported callables retain their defining module instance when
called; they do not become closures of the importing program.

The loader inserts an initializing instance into its cache before executing a
module. A later import of that identity observes the same instance, enabling
checked cyclic initialization rather than recursive host loading.

## Consequences

- Module globals cannot collide with importer globals.
- Import caching covers both compiled code and initialized state.
- The VM needs a module-call boundary for exported callables before source
  `import(...)` can be enabled.
- Use-before-initialization remains a checked module-runtime failure.

## Migration

None.
