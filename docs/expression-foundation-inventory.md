# Expression Foundation Inventory

This inventory turns roadmap milestone 2 into independently implementable source-language slices. It compares the
normative grammar and language specification with the current AST, parser, compiler, VM, and focused tests. It is an
implementation planning record, not a language specification.

The support matrix is the public status summary. This document records the implementation boundary behind its split rows
and the order in which remaining work can proceed without introducing compatibility shortcuts.

## Current implementation boundary

| Source surface                                                            | Current status              | Implementation evidence                                                       | Next work                                                                    |
|---------------------------------------------------------------------------|-----------------------------|-------------------------------------------------------------------------------|------------------------------------------------------------------------------|
| Decimal integer literals with `_` separators                              | Implemented                 | `TokenKind::Int`, `Value::Int`, CLI tests                                     | Keep checked overflow behavior.                                              |
| Floating-point, hexadecimal, and byte literals                            | Implemented                 | Lexer tokens, parser literals, existing float/byte values, and CLI tests      | Preserve checked malformed-literal diagnostics.                              |
| Double-quoted strings with `\\n`, `\\r`, `\\t`, `\\"`, and `\\\\` escapes | Implemented                 | `TokenKind::Str`, CLI tests                                                   | Preserve source locations for malformed escapes.                             |
| Raw and triple-quoted strings, `\\$`, and unknown escapes                  | Implemented                 | Lexer string scanner and CLI tests                                             | Preserve delimiter/newline behavior.                                         |
| One-to-three-digit octal escapes                                             | Implemented                 | Lexer string scanner and CLI tests                                             | Preserve source locations for unterminated strings.                          |
| `$identifier` interpolation                                                  | Implemented                 | String parts lower through normal name resolution and a private VM operation  | Keep property access and arbitrary expressions unsupported.                  |
| Lists, maps, computed map keys, and list spreads                          | Implemented                 | AST/compiler collection forms, VM collection ops, CLI/VM tests                | Keep map-key and non-list-spread failures checked.                           |
| Arithmetic, string concatenation, and list concatenation (`+`, `-`, `*`, `/`, `%`) | Implemented | Binary AST/compiler and VM arithmetic ops, plus CLI/VM tests | Preserve integer precision and checked overflow/division failures. |
| Equality, comparisons, logical-and, and logical-or                         | Implemented                 | Binary AST and VM branch/comparison ops                                       | Keep short-circuit evaluation.                                               |
| Integer bitwise, shift, and prefix `~` operators                           | Implemented                 | Source lowering and checked VM operations, plus CLI/VM tests                  | Keep pipeline as a separate slice.                                           |
| Directional list append and prepend (`:+`, `+:`)                           | Implemented                 | Source lowering and checked VM list operations, plus CLI/VM tests             | Preserve immutable-list results and checked directional-operand failures.    |
| Pipeline operator                                                          | Not implemented             | Lexer/parser/bytecode have no representation                                  | Add pipeline semantics with source and type-failure tests.                   |
| Calls, named/spread arguments, indexing, and dot access                   | Implemented                 | Source compiler, VM call binder and index operation                           | Keep call binding and collection-access diagnostics checked.                 |
| List slices                                                               | Implemented                 | Source and VM slice operations, plus CLI/VM tests                             | Preserve checked bounds and positive-step behavior.                          |
| Struct schemas, construction, defaults, and field access                  | Implemented                 | AST/compiler schema and construction forms, VM struct ops                     | Preserve schema identity and checked construction.                           |
| Struct copies                                                              | Implemented                 | Source AST/parser/compiler and VM replacement operation, plus CLI tests      | Preserve schema identity and checked replacement diagnostics.                |
| Struct patterns and field annotations                                      | Not implemented             | No AST/parser/compiler support                                                | Add patterns before annotations; annotations depend on the type slice.       |
| Declaration, parameter, return, and struct-field annotations              | Not implemented             | Grammar only; parser does not retain annotations                              | Parse annotations before optional static checking.                           |
| Tags, documentation statements, foreign declarations, and `???`           | Not implemented             | Lexer/parser lack the relevant source forms                                   | Add after annotations, with module metadata/host integration kept separate.  |

## Implementation surface by feature family

| Family                 | AST                                         | Parser/lexer                             | Compiler                                       | VM/value                                     | Primary tests                                     |
|------------------------|---------------------------------------------|------------------------------------------|------------------------------------------------|----------------------------------------------|---------------------------------------------------|
| Literals and strings   | New literal or interpolation representation | New number and string forms              | Constant lowering and interpolation sequencing | Existing float/bytes values; string assembly | `tests/cli.rs`, focused VM literal tests          |
| Operators              | New binary/prefix variants                  | Tokens, precedence, newline continuation | New private `Op` variants                      | Checked value operations                     | CLI operator behavior and VM type failures        |
| Slices                 | Slice expression/index mode                 | `start:end[:step]` bracket grammar       | Slice opcode lowering                          | Checked list slicing                         | CLI boundary cases and VM invalid-step cases      |
| Struct copy/patterns   | Copy expression and pattern form            | `copy { ... }` and schema patterns       | Construction/matching lowering                 | Struct replacement and schema-aware matching | CLI behavior and VM matching tests                |
| Annotations            | Annotation syntax tree                      | Type grammar                             | Annotation retention/checker entry points      | No runtime coercion                          | CLI accepted syntax and `-type-check` diagnostics |
| Metadata/foreign forms | Declaration metadata                        | Tags, docs, `foreign`, `???`             | Module metadata and foreign references         | Host lookup boundary                         | CLI diagnostics plus later module fixtures        |

## Dependency order

1. Add the remaining operators one family at a time, starting with numeric bitwise and shifts, then collection
   concatenation and pipeline semantics.
2. Add struct patterns before attaching field annotations.
3. Parse and retain annotations, then add the required static checks.
4. Add tags, documentation statements, foreign declarations, and `???` once their metadata and host boundaries can be
   designed alongside modules.

Every slice must update the grammar when syntax changes, add CLI coverage for accepted source and diagnostics, add VM
coverage for private execution boundaries, update `docs/language-support.tsv`, regenerate the support matrix, and run
`make check`.
