# Changelog

## Unreleased

- Extended plain `defer` cleanup to checked VM runtime faults.

- Added LIFO plain `defer` cleanup for normal returns and uncaught throws.

- Added checked language-level `throw` with the thrown Slug value, source
  location, and call frames retained by uncaught runtime errors.

- Clarified the policy requiring undocumented Slug-visible decisions to be
  recorded in their owning normative document before implementation.

- Added exact map patterns with `{| ... |}`.
- Added named `...rest` captures for non-exact map patterns.
- Added list and map destructuring for `val` and `var` declarations.
- Added function match bodies with their parameter-derived subjects.
- Added non-exact string-key map patterns for `match`.
- Added `if` guards for match cases.
- Added literal and list-pattern `match` expressions with case-local bindings.
- Preserved escaping closure captures across `recur(...)` iterations.
- Added stack-safe tail recursion through `recur(...)`.
- Clarified that Slug uses recursion, including tail-position `recur(...)`, for
  repetition and has no `while`, `for`, `loop`, `break`, or `continue` forms.
- Added explicit `return expression` for early exit from source functions.
- Removed symbols from the language value model. Bare map keys, map patterns,
  dot access, module exports, and configuration keys now use strings.
- Preserved integer precision in integer arithmetic and comparisons; rejected
  oversized bytecode calls without host overflow; and improved map dot lookup
  plus runtime frame names and call-site spans.
- Added parser nesting limits and support for `//`, `/* ... */`, and `/** ... */`
  comments.
- Fixed parser stack overflow risks for long prefix sequences and corrected
  comments, infix continuation, delimited multiline expressions, and brace
  disambiguation at source newlines.
- Added short-circuit `&&` and `||` expressions with operator-aware newline
  continuation.
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
