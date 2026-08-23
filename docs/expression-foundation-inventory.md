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
| Floating-point, hexadecimal, and byte literals                            | Not implemented             | Values can hold floats and bytes, but the lexer produces only integers        | Add lexical forms, parsed values, and literal diagnostics.                   |
| Double-quoted strings with `\\n`, `\\r`, `\\t`, `\\"`, and `\\\\` escapes | Implemented                 | `TokenKind::Str`, CLI tests                                                   | Preserve source locations for malformed escapes.                             |
| Raw/triple strings, `\\{`, octal escapes, and interpolation               | Not implemented             | No lexer tokens or AST form for string segments/interpolations                | Add string representation and left-to-right interpolation evaluation.        |
| Lists, maps, computed map keys, and list spreads                          | Implemented                 | AST/compiler collection forms, VM collection ops, CLI/VM tests                | Keep map-key and non-list-spread failures checked.                           |
| Arithmetic `+`, `-`, `*`, and `/`                                         | Implemented                 | Binary AST and VM arithmetic ops                                              | Preserve integer precision and checked overflow/division failures.           |
| Modulo `%`                                                                | Not implemented from source | The VM has a private modulo operation, but the lexer/parser do not accept `%` | Add the source token, precedence, and checked runtime coverage.              |
| Equality, comparisons, and `&&`/`                                         |                             | `                                                                             | Implemented                                                                  | Binary AST and VM branch/comparison ops | Keep short-circuit evaluation. |
| Bitwise, shift, list-concatenation, pipeline, and prefix `~` operators    | Not implemented             | Lexer/parser/bytecode have no representations                                 | Add one operator family at a time with type-failure tests.                   |
| Calls, named/spread arguments, indexing, and dot access                   | Implemented                 | Source compiler, VM call binder and index operation                           | Keep call binding and collection-access diagnostics checked.                 |
| List slices                                                               | Not implemented             | Bracket parsing accepts one expression only; VM has only `GetIndex`           | Define a slice AST/bytecode boundary and negative-index/step behavior.       |
| Struct schemas, construction, defaults, and field access                  | Implemented                 | AST/compiler schema and construction forms, VM struct ops                     | Preserve schema identity and checked construction.                           |
| Struct copies, struct patterns, and field annotations                     | Not implemented             | No AST/parser/compiler support                                                | Add copies before patterns; annotations depend on the type-annotation slice. |
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

1. Complete literal forms and interpolation, because later annotations and metadata need reliable lexical/source-span
   handling.
2. Add the remaining operators one family at a time, starting with numeric bitwise and shifts, then collection
   concatenation and pipeline semantics.
3. Add slice syntax and execution on the existing collection boundary.
4. Add struct copying and patterns before attaching field annotations.
5. Parse and retain annotations, then add the required static checks.
6. Add tags, documentation statements, foreign declarations, and `???` once their metadata and host boundaries can be
   designed alongside modules.

Every slice must update the grammar when syntax changes, add CLI coverage for accepted source and diagnostics, add VM
coverage for private execution boundaries, update `docs/language-support.tsv`, regenerate the support matrix, and run
`make check`.
