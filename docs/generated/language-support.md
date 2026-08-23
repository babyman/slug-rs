# Language Support Matrix

Generated from `docs/language-support.tsv`; do not edit directly.

| Feature | Status | Evidence |
|---|---|---|
| Lexical bindings and assignment | implemented | `tests/cli.rs` |
| List and map destructuring declarations | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Decimal integer literals with `_` separators | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Floating-point, hexadecimal, and byte literals | implemented | `tests/cli.rs` |
| Double-quoted strings with basic escapes | implemented | `tests/cli.rs` |
| Raw and triple-quoted strings with basic escapes | implemented | `tests/cli.rs` |
| One-to-three-digit octal escapes | implemented | `tests/cli.rs` |
| Interpolated strings | specified only | `language/Strings - Mini Spec.md` |
| Boolean and nil literals | implemented | `tests/cli.rs` |
| Lists and maps | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Arithmetic `+`, `-`, `*`, and `/` | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Modulo `%` | specified only | `language/language-specification.md` |
| Equality and comparisons | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Bitwise, shift, list-concatenation, and pipeline operators | specified only | `language/language-specification.md` |
| Short-circuit logical-and and logical-or | implemented | `tests/cli.rs` |
| Functions, blocks, conditionals, and captures | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Literal and list pattern matching | implemented | `tests/cli.rs` and `tests/vm.rs` |
| `name @ pattern` whole-value bindings | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Pinned `^name` patterns | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Non-binding match-case alternatives | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Non-exact string-key map patterns | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Computed map-pattern keys | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Named map rest captures | implemented | `tests/cli.rs` |
| Anonymous list and map rest patterns | implemented | `tests/cli.rs` |
| Exact map patterns | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Match guards | implemented | `tests/cli.rs` |
| Function match bodies | implemented | `tests/cli.rs` |
| Explicit function return | implemented | `tests/cli.rs` |
| Language-level `throw` with checked payloads | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Plain `defer` cleanup on returns and errors | implemented | `tests/cli.rs` |
| `defer onsuccess` cleanup | implemented | `tests/cli.rs` |
| `defer onerror` cleanup and recovery | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Tail-position `recur(...)` with ordinary call binding | implemented | `tests/cli.rs` and `tests/vm.rs` |
| List, map, and dot indexing | implemented | `tests/cli.rs` and `tests/vm.rs` |
| List slicing | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Untyped struct schemas, construction, defaults, and field access | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Struct copies and patterns | specified only | `language/language-specification.md` |
| Native function calls and `println` | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Positional call spreads and list-literal spreads | implemented | `tests/cli.rs` |
| Named ordinary-function arguments | implemented | `tests/cli.rs` |
| Final variadic parameters | implemented | `tests/cli.rs` |
| Call-time default parameters | implemented | `tests/cli.rs` |
| Type annotations and required static checks | specified only | `language/language-specification.md` |
| Tags, documentation statements, foreign declarations, and `???` | specified only | `language/language-specification.md` |
| Checked source and runtime diagnostics with locations | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Full language specification | specified only | `language/language-specification.md` |
| Modules, standard library, and concurrency | specified only | `language/runtime-requirements.md` |
