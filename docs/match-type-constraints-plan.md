# Match Type-Constraints Implementation Plan

This document plans the implementation of whole-case match type constraints.
It is not a language specification; the accepted source rule is defined by
[Match and Destructuring](language/Match%20and%20Destructuring%20-%20Mini%20Spec.md)
and its rationale by
[Constrain whole match patterns with types](decisions/2026-08-29-typed-match-patterns.md).

## Scope and invariants

The feature accepts a type constraint only after a complete match-case pattern:

```slug
usr @ {age: 43, name}: struct<User> => name
```

The implementation must preserve these rules:

- type constraints apply to whole case patterns, never declarations or nested
  patterns;
- a failed constraint is an ordinary failed match and proceeds to the next
  case;
- type constraints run before a case guard and bindings become visible only on
  a successful case;
- `struct<User>` compares the exact runtime schema identity, while `_: struct`
  accepts any struct;
- runtime checking supports direct value categories, `any`, unions,
  `struct<Name>`, and recursive `list<T>` and `map<K, V>`;
- function signatures, task/channel payloads, tuple types, and generic
  parameters are rejected as non-reifiable case constraints; and
- invalid source remains a `SourceError` and runtime schema misuse remains a
  checked `RuntimeError`, never a host panic.

The work deliberately does not add exhaustiveness checking, nested type
constraints, runtime function/task/channel type metadata, or coercion.

## 1. Source AST and parser

- [ ] Add `constraint: Option<TypeAnnotation>` to `MatchCase` in
  `src/source/ast.rs`.
- [ ] Change `Parser::match_cases` in `src/source/parser.rs` to parse an
  optional `: type_annotation` after each complete case pattern and before its
  guard or arrow.
- [ ] Preserve the existing non-binding restriction for comma-separated case
  alternatives. Each alternative may carry its own whole-pattern constraint.
- [ ] Remove source parsing of the superseded `Schema {field}` pattern form.
- [ ] Add parser diagnostics proving that declaration annotations and
  map-pattern entries remain unambiguous.

## 2. Semantic validation and match representation

- [ ] Add one private match-constraint representation to `src/bytecode.rs`.
  It must represent direct value categories, `any`, unions, recursive list/map
  checks, and a schema operand for `struct<Name>` without making source types
  or bytecode encoding public compatibility promises.
- [ ] Add a semantic helper that resolves a constraint annotation and rejects
  non-reifiable forms in every compiler mode, not only with `-type-check`.
- [ ] Resolve the `Name` in `struct<Name>` through the same lexical operand
  mechanism used by the former struct pattern. Unknown names are source
  errors; a resolved non-schema value is a runtime type error.
- [ ] Extend `lower_case_patterns` and its callers in `src/source/compiler.rs`
  to lower both the structural pattern and its constraint, collecting dynamic
  schema operands in deterministic source order.
- [ ] Replace the source use of `MatchPattern::Struct`; remove the obsolete
  AST, compiler, and bytecode form once its focused VM coverage has migrated.

## 3. VM matching

- [ ] Extend `matches_pattern` in `src/vm/operations.rs` so a constraint is
  tested before its structural pattern and rolls back pending bindings on
  failure.
- [ ] Implement direct checks for `nil`, `bool`, `num`, `str`, `bytes`,
  `list`, `map`, `fn`, `task`, `chan`, and `struct`, plus the non-nil `any`
  rule and union alternatives.
- [ ] Implement exact schema-identity matching for `struct<Name>`.
- [ ] Implement recursive element checks for `list<T>` and recursive key/value
  checks for `map<K, V>`, including every entry outside or inside a rest
  binding.
- [ ] Preserve `TryMatch`'s binding-count verification and malformed-bytecode
  failures when constraints refer to invalid operand indexes.

## 4. Optional checker narrowing

- [ ] Analyze each match case in a child `Environment` in
  `src/source/typecheck.rs`.
- [ ] Narrow the match subject and every binding introduced by `@`, list/map
  patterns, and rests using the successful constraint.
- [ ] Make guards and case results use that child environment; do not leak
  narrowed bindings or subject information to later cases or the surrounding
  scope.
- [ ] Retain conservative `unknown` behavior when a structural pattern cannot
  prove a more precise element, field, or rest type.
- [ ] Add `-type-check` tests demonstrating accepted narrowed calls and
  rejected incompatible calls inside a case.

## 5. Migration and regression coverage

- [ ] Replace existing source tests using `User {name}` with
  `{name}: struct<User>` and preserve their schema-identity, partial-field,
  duplicate-field, and invalid-schema coverage.
- [ ] Add CLI tests for primitive cases, any-struct cases, exact maps with
  `map<str, str>`, recursive lists/maps, unions, pipeline matches, and guards.
- [ ] Add CLI diagnostics for nested constraints, non-reifiable constraints,
  and unresolved schema names.
- [ ] Add VM tests for private constrained-pattern bytecode, binding rollback,
  bad schema operands, and recursive collection matching.

## 6. Documentation and verification

- [ ] Mark whole-case type constraints implemented in
  `docs/language-support.tsv` and regenerate the support matrix.
- [ ] Update the README capability statement and replace the temporary
  changelog wording that says the feature is specified only.
- [ ] Remove the compatibility note describing the former struct-pattern
  spelling as temporarily implemented.
- [ ] Run the narrow parser, CLI, type-checker, and VM tests while iterating.
- [ ] Run `make check` before handoff.

## Completion criteria

The feature is complete when the new source spelling is the only supported
struct-match spelling, all accepted reifiable constraints have source and VM
coverage, invalid constraints have checked diagnostics, `-type-check` narrows
case-local bindings, the support matrix reports implementation, and
`make check` passes.
