# Changelog

## Unreleased

- Added shared mutable binding cells so closures and sibling closures observe
  assignments to captured `var` bindings.
- Added a span-aware Rust source frontend for lexical bindings, functions and
  captures, blocks, conditionals, comparisons, and list/map indexing.
- Added source-located parse and semantic diagnostics plus source spans on
  emitted runtime instructions.
- Adopted a portable `.cslug` compiled-module compatibility contract and
  documented the required versioning, validation, and implementation gate.
- Added the Rust Slug bytecode VM foundation.
- Added initial source parsing, bytecode compilation, and CLI execution for
  bindings, assignments, literals, arithmetic, calls, comments, and `println`.
- Added Codex-focused repository guidance, language-change workflows, local
  validation targets, and continuous integration for agentic development.
- Moved language documentation under `docs/`, added scoped documentation
  guidance, and added an automatically checked language support matrix.
