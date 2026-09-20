# Body-Derived Parameter Inference

This is the implementation task list for the rule adopted in
[Infer unannotated parameters from function bodies](../decisions/2026-09-20-body-derived-parameter-inference.md).
It does not permit caller-derived inference or function specialization by call
site.

## Scope and invariants

- An explicit parameter annotation is authoritative.
- Constraints originate only in the declaring function body, including its
  defaults and valid `recur` expressions; callers only validate the solved
  signature.
- The first domain infers only `num` from division, modulo, unary negation, and
  ordinary ordering comparisons. It does not infer from overloaded `+`, `-`,
  or `*`.
- An unresolved or conflicting parameter remains `any|nil` unless the body
  proves an actual static contradiction.
- Inferred signatures participate in overload identity, source/module metadata,
  structural function types, and direct-call checking. Runtime values remain
  uncoerced and dynamically callable.

## Task list

### 1. Represent local parameter constraints

- [x] Add an internal, function-scoped constraint state for each unannotated,
  non-discard parameter.
- [x] Record a `num` requirement when that parameter occurs directly in a
  division, modulo, unary-negation, or ordinary ordering operand.
- [x] Keep explicit annotations and variadic container types outside the
  inferred-parameter domain in this first stage.
- [x] Define conflict handling and source-span selection for incompatible
  requirements before emitting a diagnostic.

### 2. Solve a function body before publishing its signature

- [x] Construct provisional local bindings for the body without consulting any
  caller types.
- [x] Collect constraints through nested blocks, conditionals, defaults, and
  tail-recursive expressions; do not let nested function constraints escape.
- [x] Solve the body to a stable parameter vector, then recheck the body under
  that vector so ordinary expression facts and diagnostics agree with the
  signature.
- [x] Preserve `any|nil` for parameters with no unique solved type.

### 3. Publish the solved callable metadata

- [x] Build callable identity from solved parameter types, so inferred `num`
  and explicit `num` signatures collide as duplicates.
- [x] Use the solved signature for direct-call applicability, overload ranking,
  structural function values, selected-call metadata, and export snapshots.
- [x] Ensure imports consume exported solved signatures rather than inferring
  imported functions again.
- [x] Keep live-binding checks and runtime argument binding shape-only; this
  stage adds no runtime annotation validation.

### 4. Feed solved facts to lowering

- [x] Record solved parameter types in expression facts for their name uses.
- [x] Verify numeric arithmetic and relational lowering selects existing
  checked numeric opcodes when a body-derived fact proves both operands `num`.
- [x] Keep overloaded operators generic until they have an explicit relational
  constraint rule.

### 5. Prove behavior

- [x] Add CLI coverage for accepted numeric body inference and rejected known
  incompatible calls.
- [x] Add coverage that `fn(value) { value + 10 }` remains broad and accepts a
  string call, while `fn(value) { value / 10 }` does not.
- [x] Add recursive, nested-function, default-parameter, overload-collision,
  structural-function-value, module-export, and import cases.
- [x] Add dynamic-call coverage proving an unresolved runtime value retains its
  checked runtime failure instead of a host panic.
- [x] Add VM/lowering coverage for inferred numeric opcode selection and
  rerun the typed/untyped source benchmark pairs.

### 6. Complete the language change

- [x] Update `docs/language-support.tsv`, regenerate the support matrix, and
  update the README capability statement when implementation lands.
- [x] Add a changelog entry for the implemented semantics, not merely this
  plan.
- [x] Run focused semantic, CLI, module-loader, and VM tests during the work;
  run `make check` before handoff.
