# Bind imported closures to their defining module

## Context

A closure previously stored only a private bytecode chunk index. Calling an
imported closure therefore interpreted that index against the importing
program, and global reads resolved in the importing module rather than the
defining module.

## Decision

Closures created while evaluating a module retain an internal reference to
their defining compiled program and a clone of that module's shared global
binding environment. A call from a different program executes the closure in
a child VM using that retained context. Calls within the same module retain the
ordinary VM-frame path.

## Consequences

- Imported functions execute their own bytecode and observe live exported
  bindings, including changes to exported `var` values.
- Cyclic modules may safely export functions that refer to the other module
  after both modules initialize.
- Cross-module callable overload composition remains separate work; it needs
  an explicit callable-set representation rather than chunk-index dispatch.

## Migration

None.
