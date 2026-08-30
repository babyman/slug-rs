# Type-System Implementation Plan

This document is the dependency-ordered plan for making Slug's optional static
checking more useful without changing its dynamic execution model. It does not
define source-language semantics. Each stage must establish its user-visible
rules in the normative language documents—and add a decision record where the
choice is durable—before implementation begins.

## Current foundation

The checker already understands built-in value types, unions, generic function
calls, structural function values, parameterized collections and concurrency
handles, first-class `schema` values, nominal `struct<S>` values, and
case-local narrowing from whole-case match constraints. Annotations remain
static only: they neither coerce a value nor add runtime validation.

The remaining gap is chiefly the checker’s ability to derive and preserve
facts from ordinary expressions and control flow. The stages below prioritize
that capability over adding more annotation forms.

## Scope and invariants

- Preserve `-type-check` as optional additional diagnostics; programs accepted
  without it retain their current runtime behavior.
- Report only contradictions the checker can prove. Incomplete information
  widens conservatively rather than rejecting valid dynamic programs.
- Keep type facts lexical and flow-sensitive. Facts from one conditional
  branch, match case, loop-like construct, or function invocation must not
  escape without a sound merge rule.
- Keep source semantics in `docs/language/`, implementation detail in Rust,
  and bytecode private.
- Add CLI coverage for source behavior and diagnostics; use VM tests only when
  a stage changes private bytecode/runtime behavior.
- Do not introduce runtime type tags, coercion, reflection, or exhaustiveness
  errors incidentally as part of static checking.

## 1. Checked operations and expression results

Make inferred types useful at the expression boundaries Slug already has.

- [x] Specify operand and result rules for prefix operators, arithmetic,
  bitwise operations, comparisons, equality, directional list operations,
  indexing, slicing, and interpolation. Field-level rules belong to stage 4.
- [x] In strict mode, reject provably invalid operand combinations with a
  source diagnostic at the offending expression; preserve dynamic behavior for
  `unknown` and sufficiently broad union operands.
- [x] Derive precise result types where the operation proves them: for example
  `num` arithmetic, `str` interpolation, list element lookup, map value
  lookup, and list slicing.
- [x] Preserve list/map precision through supported spreads; deliberately
  widen only when a spread's type cannot prove its element or key/value type.
- [ ] Use nominal schema information to type-check statically known struct
  construction fields, copy replacements, and direct field access where the
  schema declaration is available.
- [ ] Add focused tests for accepted typed operations, static failures, union
  operands, and dynamic/unknown fallbacks.

## 2. Control-flow narrowing and environment joins

Generalize the case-local narrowing mechanism into reusable flow analysis.

- [x] Define narrowing predicates for `value != nil`, `value == nil`,
  type-constrained `match`, and short-circuit `&&`/`||`; document
  which comparisons intentionally do not refine a type.
- [x] Represent positive and negative facts without exposing the checker's
  private `unknown` state in source types.
- [x] Analyze `if` branches using child environments and merge binding types
  only when both continuing branches define compatible facts.
- [x] Apply short-circuit facts to the right operand of `&&` and `||`, then
  discard branch-local facts afterward unless an explicit join preserves them.
- [x] Reuse the same narrowing and join rules for match guards and case-result
  inference so those paths do not diverge.
- [x] Add source tests for nilable values, nested conditions, shadowing,
  assignments, and intentionally non-refining predicates.

## 3. Match coverage and unreachable cases

Use the types and narrowing facts from the first two stages to improve match
diagnostics without changing match's runtime first-match semantics.

- [x] Specify the supported coverage domain: closed unions, direct value
  categories, `schema`, and nominal `struct<S>` identities; leave open maps,
  lists, arbitrary guards, and dynamic values conservative.
- [x] Diagnose a case whose type constraint is disjoint from the remaining
  statically known subject type.
- [x] Diagnose a case made unreachable by an earlier unguarded wildcard or
  equivalent covered type constraint.
- [x] Diagnose non-exhaustive matches only when the subject's coverage domain
  is closed and the checker can name the remaining type(s); otherwise preserve
  the runtime `nil` result without a diagnostic.
- [x] Treat a guard as potentially false unless it can be proved true; it must
  not make following cases unreachable by itself.
- [x] Add CLI tests for covered/uncovered unions, schema identities, guarded
  cases, wildcard cases, and intentionally conservative collection patterns.

## 4. Schema-aware records and nominal-reference integrity

Finish the static record model now that schemas and `struct<S>` exist.

- [x] Require `struct<S>` to resolve statically to a
  schema binding at every annotation site, including imports, aliases, and
  shadowed names; define the diagnostic when it does not.
- [x] Retain schema field declarations in semantic metadata suitable for type
  checking, without making them a public bytecode representation.
- [x] Validate known-schema construction: required fields, unknown fields,
  supplied values, defaults, and spread/copy behavior where statically
  provable.
- [x] Infer field-access and copy results from nominal schemas while retaining
  generic `struct` behavior for dynamically selected schemas.
- [x] Define how aliases preserve schema identity and how separately created
  lookalike schemas remain distinct.
- [x] Cover local, imported, aliased, and dynamically selected schema cases in
  CLI and module-loader tests.

## 5. Deliberate language-surface extensions

Only after expression checking and flow analysis have stabilized, decide which
new abstractions materially improve Slug programs.

- [x] Evaluate named closed variants (enums/sum types) as the next candidate
  for typed domain modelling. The proposed constructor, payload, match,
  nominal-identity, coverage, import, and runtime design is recorded in
  [the deferred ADR](decisions/2026-08-29-named-closed-variants.md); no
  implementation is scheduled by this plan.
- [ ] Evaluate type aliases as a readability feature independently of nominal
  types; define expansion, diagnostics, export behavior, and cycle handling
  before adding syntax.
- [ ] Evaluate richer function typing—labels, defaults, variadics, effects,
  task results, and channel payload flow—only where static callable metadata
  can represent it without changing dynamic call behavior.
- [ ] Defer generic bounds, variance, higher-kinded types, runtime reflection,
  and coercion unless a concrete language feature requires them.

## Verification and handoff

Each completed stage must update its normative language documentation, the
EBNF only when syntax changes, `docs/language-support.tsv`, the generated
support matrix, README capability statement when applicable, and
`changelog.md`. During implementation, run focused CLI/type-checker tests;
before handoff, run `make check`.

## Completion criteria

The plan is complete when Slug can soundly type-check the ordinary expressions
and control flow that its existing annotations describe, reports clearly
provable match coverage mistakes, gives nominal schemas useful field-level
precision, and records any chosen new type constructs as separate language
decisions rather than implicit checker behavior.
