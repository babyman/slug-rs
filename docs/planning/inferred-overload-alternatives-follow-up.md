# Inferred Overload Alternative Follow-Up

This plan records work deliberately deferred after the finite `+` alternatives
implemented in
[Finite Inferred Overload Alternatives](completed/finite-inferred-overload-alternatives.md).
It does not reopen the rule that function bodies, rather than callers, derive
their alternatives.

Known-direct-call constraints and nil-partitioned parameter inference are
tracked separately in [Body-Derived Inference Follow-Up](body-derived-inference-follow-up.md).
They may supply singleton body facts to a future alternative family, but do not
expand structural-call preservation or introduce specialization.

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

### Additional built-in operator families

- [ ] Specify and implement subtraction alternatives: numeric subtraction and
  map-key removal, including the static map-key domain.
- [ ] Specify and implement multiplication alternatives: numeric multiplication
  and string repetition, including the VM's checked integral repetition rule.
- [ ] Evaluate whether any other existing overloaded operations have a finite,
  useful relational scheme set. Do not add alternatives merely because an
  operator has more than one runtime branch.

### Alternative composition and callable precision

- [ ] Define composition when several overloaded operations constrain the same
  parameters across branches, nested functions, defaults, and `recur`.
- [ ] Decide whether structural function values can retain alternative sets
  without exposing a new source-level constrained-function type syntax.
- [ ] Define overlap, identity, and ambiguity rules between explicit overloads,
  body-derived singleton signatures, and inferred alternative sets.
- [ ] Establish practical limits and diagnostics for alternative-set growth.

### Runtime and performance boundaries

- [ ] Decide whether inferred alternatives may ever select specialized bytecode
  at a known call boundary, and how that preserves behavior for dynamic calls.
- [ ] If specialization is adopted, compare a guarded fast path with explicit
  function-entry contracts; do not let static alternatives silently alter
  dynamic operator behavior.
- [ ] Add paired source benchmarks only after an optimization has a runtime-safe
  design and a representative workload.

### Verification and documentation

- [ ] Add CLI, module-loader, and VM coverage for each accepted operator family
  and for rejected mixed operand pairs.
- [ ] Update the language specification, support inventory, generated matrix,
  README, changelog, and any required decision record with every new family.
- [ ] Run `make check` before closing each follow-up stage.
