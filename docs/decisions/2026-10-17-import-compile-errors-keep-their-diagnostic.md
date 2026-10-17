# Import compile errors keep their diagnostic

## Status

Accepted

## Context

Imports run when the importing code runs, so a module whose source does not
compile fails as a runtime `module` error at the `import` call. That error
held only text: the module's parse or semantic error, with its code, range,
expected and actual values, and fixes, was flattened into the message by each
layer it crossed, from the frontend through the loader and VM host to the
runtime error. Agents commonly test a library by importing it from a test
program; in one JSON-library evaluation run, 9 of 30 failed runs were this
error, so the structured diagnostics never reached the agent on its main
testing path. See the [agent-facing direction](../planning/agent-facing-direction.md).

## Decision

- `slug-vm` defines `SourceDiagnostic`, a source error as plain data, beside
  the position, range, edit, and fix types, which move there from the
  frontend. The frontend re-exports them and converts its `SourceError` into
  one.
- `slug-loader` depends on `slug-vm` for that type. `ModuleLoadError` gains a
  `Compile` variant carrying it; `Source` remains for modules that compiled but
  failed to install or initialize. `VmHostError` and `RuntimeError` carry the
  diagnostic of a module that failed to compile.
- The diagnostic document's `cause` for such an import is the module's own
  `parse` or `semantic` diagnostic, in the same form as for an entry file, so
  its fixes apply to the file named by its `location`. Text output shows it
  with its source line under `caused by:`.
- When a module's own import fails to compile, it reports the innermost
  failing module's error, which is the one to fix.

## Consequences

### Positive

- An agent sees the same code, range, and fixes whether it runs a module
  directly or through an import.

### Negative

- The loader's error type now depends on the VM crate.

### Neutral

The `module` diagnostic's code and message are unchanged.

## Migration

Rust code matching `ModuleLoadError` exhaustively handles `Compile`.
