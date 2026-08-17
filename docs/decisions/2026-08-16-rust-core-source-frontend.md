# Build the core source frontend in Rust

## Context

Slug's VM already has a checked execution core, but its initial source frontend
only covered top-level arithmetic and calls.  It dropped source locations,
treated `val` as mutable, and could not expose the VM's existing closure and
branching capabilities through source programs.

The language grammar and runtime requirements specify a much wider language,
but a native-Slug lexer/parser would require completing a large bootstrap
language before it could help implement that language.

## Decision

The Rust frontend remains the authoritative implementation frontend while the
core language is completed.  It now carries source spans through lexing,
parsing, and bytecode lowering, and implements this source-level core:

- immutable and mutable lexical bindings, including block scope;
- function literals, positional calls, and value captures;
- blocks, `if` expressions, arithmetic, equality, comparisons, and `!`;
- list and map literals plus list, map, and dot indexing.

Parse and compile-time semantic failures are distinct diagnostic categories and
include source locations.  Runtime instructions emitted from source carry the
corresponding span.

The deliberately narrow subset excludes patterns, type annotations, function
defaults/variadics/named arguments, slices, map mutation, and assignment to a
captured `var`.  Those features must be implemented to their language rules,
not inferred from the core implementation.

## Consequences

- The source frontend is an executable oracle for later native-Slug frontend
  work, rather than a bootstrap obstacle.
- The VM gains internal list/map construction and indexed-access operations,
  with validation that preserves checked runtime failures.
- Source and CLI tests must cover public syntax, semantic diagnostics, and
  runtime location propagation for every subsequent frontend extension.

## Migration

Existing programs using the documented initial subset continue to work.  A
source assignment to a `val` binding now correctly fails with a semantic
diagnostic instead of being accepted as a mutable global assignment.
