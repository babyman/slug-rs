# Inferred Overload Alternative Follow-Up

This plan records work deliberately deferred after the finite `+` alternatives
implemented in
[Finite Inferred Overload Alternatives](completed/finite-inferred-overload-alternatives.md).
It does not reopen the rule that function bodies, rather than callers, derive
their alternatives.

Known-direct-call constraints and nil-partitioned parameter inference are
tracked separately in [Body-Derived Inference Follow-Up](body-derived-inference-follow-up.md).
They now supply singleton facts through a direct, non-generic known callee
whose signature has no inferred alternatives. Extending that propagation to a
uniquely selected inferred alternative is the first remaining semantic stage.
Neither work expands structural-call preservation or introduces specialization.

## Scope and invariants

- Extend one operator family at a time, with a normative operand/result rule
  and a finite symbolic scheme set before implementation.
- Preserve parameter correlations; never approximate related operands with
  independent unions.
- Keep one source body and generic runtime behavior unless a separate decision
  establishes runtime-safe specialization or function-entry contracts.
- Keep dynamic and structural function calls valid when alternative metadata is
  unavailable.

## Deferred work

### Direct inferred-alternative propagation

- [x] When a direct known call has ordinary bound arguments and exactly one
  inferred alternative applies, propagate that alternative's parameter
  constraints and result fact into the enclosing function body. Selection uses
  only independently established body facts; it does not use the propagated
  constraints themselves.
- [x] Preserve dynamic behavior when the callee is structural or dynamic, an
  argument is spread, argument binding is incomplete, or zero/multiple
  alternatives apply. A bare unannotated wrapper does not forward the callee's
  correlated alternative set; that is deferred composition work.
- [x] Add CLI and module-loader coverage for a valid wrapper, a rejected mixed
  pair at a known wrapper call, and a dynamic wrapper call that retains its
  checked runtime failure.

### Additional built-in operator families

- [x] Specify and implement subtraction alternatives: numeric subtraction and
  map-key removal, including the static hashable map-key domain. Map removal
  preserves `map<K,V>` without unifying `K` with the removal operand.
- [x] Specify and implement multiplication alternatives: numeric multiplication
  and string repetition, retaining the VM's checked integral repetition rule.
- [x] Evaluate other existing overloaded operations. Division, modulo,
  comparisons, and shifts are numeric-only; bitwise byte coercion and
  directional collection updates depend on runtime value bounds. None adds a
  useful finite correlated scheme beyond `+`, `-`, and `*` at this stage.

### Alternative composition and callable precision

- [x] Define composition when several overloaded operations constrain the same
  parameters across branches, nested functions, defaults, and `recur`; see
  `2026-09-20-inferred-alternative-composition.md`.
- [x] Make the current composition boundary explicit in tests: alternatives are
  initially derived from one operator family and direct parameter pair; distinct
  families do not compose or silently approximate relationships with unions.
- [x] Preserve widening at structural and dynamically selected function-value
  boundaries. Reconsider retention of alternatives there only through a new
  decision record with a concrete callable representation.
- [x] Apply the existing canonical identity, specificity, and ambiguity rules
  to every added family; add a new rule only when a concrete overlap cannot be
  resolved by those rules.
- [x] Establish practical limits, widening behavior, and diagnostics for
  alternative-set growth when cross-expression composition can actually grow
  a set beyond one operator family's finite schemes.

### Deferred runtime and performance research

- [x] Do not begin this section as a consequence of semantic alternative
  propagation. A selected alternative is not a specialization proof.
- [ ] If a concrete performance hypothesis remains after composition is stable,
  establish a separate runtime-safe design that preserves live bindings and
  dynamic-call behavior before considering private bytecode changes.
- [ ] Add paired source benchmarks only for that concrete design and a
  representative workload; retain no optimization without a measurable gain.

### Verification and documentation

- [x] Add CLI, module-loader, and VM coverage for each accepted operator family
  and for rejected mixed operand pairs.
- [ ] Update the language specification, support inventory, generated matrix,
  README, changelog, and any required decision record with every new family.
- [ ] Run `make check` before closing each follow-up stage.
