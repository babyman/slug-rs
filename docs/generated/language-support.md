# Language Support Matrix

Generated from `docs/language-support.tsv`; do not edit directly.

| Feature | Status | Evidence |
|---|---|---|
| Lexical bindings and assignment | implemented | `tests/cli.rs` |
| List and map destructuring declarations | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Literals: integers, strings, booleans, nil, lists, and maps | implemented | `tests/cli.rs` |
| Arithmetic and comparisons | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Short-circuit logical `&&` and ` |  | `|implemented|`tests/cli.rs` |
| Functions, blocks, conditionals, and captures | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Literal and list pattern matching | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Non-exact string-key map patterns | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Named map rest captures | implemented | `tests/cli.rs` |
| Exact map patterns | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Match guards | implemented | `tests/cli.rs` |
| Function match bodies | implemented | `tests/cli.rs` |
| Explicit function return | implemented | `tests/cli.rs` |
| Language-level `throw` with checked payloads | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Scoped `defer` cleanup and recovery | specified only | `language/language-specification.md` |
| Tail-position `recur(...)` | implemented | `tests/cli.rs` and `tests/vm.rs` |
| List, map, and dot indexing | implemented | `tests/cli.rs` |
| Native function calls and `println` | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Checked source and runtime diagnostics with locations | implemented | `tests/cli.rs` and `tests/vm.rs` |
| Full language specification | specified only | `language/language-specification.md` |
| Modules, standard library, and concurrency | specified only | `language/runtime-requirements.md` |
