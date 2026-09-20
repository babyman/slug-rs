# Finite Inferred Overload Alternatives

This plan implements
[Infer finite overload alternatives from operator bodies](../decisions/2026-09-20-finite-inferred-overload-alternatives.md).
It extends completed body-derived numeric inference without allowing callers to
derive or mutate a function's alternatives.

## Scope and invariants

- An alternative is a symbolic parameter-and-result scheme, not independent
  union types for each parameter.
- The initial operator is `+` and its five existing runtime families: numeric,
  string concatenation, list concatenation, bytes concatenation, and map merge.
- Generic variables are fresh for each alternative and preserve relationships
  between parameters and results.
- Body constraints only remove or refine alternatives; calls only instantiate,
  select, and validate them.
- Shared function bodies retain generic runtime operator dispatch. This stage
  adds no entry guards, coercions, or caller-selected bytecode specialization.
- Alternatives are retained for statically known direct and imported callables.
  Structural or dynamic function values widen to dynamic call behavior.

## Task list

### 1. Model symbolic alternatives

- [x] Add private semantic types for an inferred alternative, scoped symbolic
  variables, and parameter/result relationships.
- [x] Keep those variables distinct from `unknown`, `any`, source generics,
  and ordinary `Type` unions.
- [x] Define substitution, canonicalization, equality, and diagnostic rendering
  for alternatives without making them bytecode or source syntax.

### 2. Derive `+` alternatives from function bodies

- [ ] Add the five `+` schemes recorded in the ADR.
- [ ] Instantiate fresh symbols for every operator occurrence.
- [ ] Intersect an operator's schemes with solved singleton body facts and with
  constraints from other operations in the same function.
- [ ] Diagnose a body only when all alternatives are impossible; retain a broad
  dynamic signature when more than one unsupported relationship remains.

### 3. Publish and select alternatives

- [ ] Retain a single source callable implementation with its alternative set;
  do not synthesize duplicate source declarations or bytecode chunks.
- [ ] Extend callable metadata and exported module snapshots to retain the set.
- [ ] At a statically known direct call, instantiate alternatives against actual
  argument types and use existing applicability/specificity rules to require a
  unique match.
- [ ] Define duplicate identity and ambiguity behavior when explicit overloads
  overlap inferred alternatives.

### 4. Preserve dynamic execution boundaries

- [ ] Widen alternatives deliberately when a function is used as a structural
  or otherwise dynamically selected function value.
- [ ] Keep VM call binding shape-only and lower the shared body with generic
  `Add` until a separate runtime-safe specialization decision exists.
- [ ] Ensure imports use exported alternatives rather than re-inference from
  importing call sites.

### 5. Prove the feature

- [ ] Add CLI tests for numeric, string, list, bytes, and map `+` calls through
  one inferred callable body.
- [ ] Prove correlated rejection of invalid mixed pairs such as `(num, bytes)`.
- [ ] Cover intersections with body-derived numeric constraints, recursive
  bodies, nested functions, explicit overloads, imports, and live bindings.
- [ ] Cover generic-variable result precision for lists and maps.
- [ ] Cover structural/dynamic fallback and checked runtime failures.

### 6. Complete the change

- [ ] Update the language specification, language-support inventory, generated
  support matrix, README capability statement, and changelog when implemented.
- [ ] Run focused semantic, CLI, module-loader, and VM tests during the work;
  run `make check` before handoff.
