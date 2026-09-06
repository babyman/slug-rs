# Unify source type checking

## Context

Slug previously exposed optional static checking through `-type-check` and
parallel compiler entry points. Semantic facts now control callable selection,
nominal identities, and diagnostics, so separate checked and unchecked source
compilation paths risk divergent language behavior.

## Decision

Every source compilation performs semantic type checking. A directly provable
contradiction is a source error; incomplete knowledge remains dynamic and the
VM retains its runtime validation. The CLI flag and APIs selecting an unchecked
source-compiler path are removed. Imported modules remain lazy, but are checked
when the module loader compiles them.

This supersedes the optional-checking portions of the 2026-08-28 static
overload-selection and the 2026-08-29 expression-operation, nil-narrowing,
schema-field, and match-coverage records.

## Consequences

Source callers share one semantic-analysis result before bytecode lowering.
Annotations remain non-coercive and do not create a general runtime type-check
system. Structural function values retain dynamic arity when their type lacks
default or variadic metadata.

## Migration

Programs that relied on the unchecked path must correct contradictions already
diagnosed by the former `-type-check` mode. Invoke the CLI as `slug program.slug`.
