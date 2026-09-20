# Body-Derived Inference Follow-Up

This task list implements the semantic decision in
[Propagate known calls and nil partitions in body-derived inference](../decisions/2026-09-20-body-derived-call-and-nil-partition-inference.md).
It extends the completed direct numeric and finite `+` work without allowing
callers to define a function's signature.

## Scope and invariants

- A function body may use an already-known direct callee signature to constrain
  one of its own unannotated parameters.
- A direct `parameter == nil` or `parameter != nil` test partitions requirements
  by reachable path and joins them conservatively.
- Dynamic or structural calls, spreads, ambiguous overloads, and unsupported
  recursive cycles widen rather than create a speculative constraint.
- Inference remains compile-time-only. It adds no runtime argument validation,
  coercion, or specialized call behavior.

## 1. Lock the source-level behavior with failing tests

- [x] Add CLI coverage for a direct wrapper:

  ```slug
  val divide = fn(value) { value / 10 }
  val apply = fn(value) { divide(value) }
  println(apply(20))
  ```

  Assert successful output and a source error for `apply("text")`.
- [x] Add CLI coverage for `if (value == nil) { 0 } else { value / 10 }` and
  the reversed `!= nil` shape. Assert that `nil` and a number succeed, while a
  statically known string is rejected.
- [x] Preserve a dynamic-call regression: a dynamic value passed to either
  function remains valid at compile time and fails through a checked runtime
  diagnostic only if its runtime value is incompatible.
- [x] Add focused semantic unit tests for inferred parameter and result types;
  avoid asserting private representations through public CLI output.

## 2. Add known-direct-call constraints

- [x] Make parameter-constraint collection aware of the lexical semantic
  environment and already-published callable signatures.
- [x] Bind ordinary positional and named arguments with the same shape rules as
  normal call resolution, then constrain a direct unannotated parameter by the
  uniquely resolved callee input type.
- [x] Use an inferred alternative only when its selection is unique; otherwise
  retain the ordinary dynamic boundary.
- [x] Infer the wrapper result from the selected callee result while preserving
  the existing result-flow rules.
- [x] Stage signature collection before strict body-call validation so
  `divide(value)` can establish `value:num` rather than fail as `any|nil`.
- [x] Do not obtain a callee constraint from dynamic/structural values, spread
  arguments, missing/default-bound positions without a proven relation, or an
  unresolved overload set.

## 3. Add nil-partitioned requirements

- [x] Represent a direct parameter-to-`nil` comparison as positive and negative
  input partitions alongside existing flow facts.
- [x] Collect numeric and callable constraints in the narrowed branch only;
  join each reachable parameter partition after the conditional.
- [x] Include the `nil` partition even when its branch terminates: it remains a
  valid input domain for the function.
- [x] Support the direct `== nil` and `!= nil` forms first. Leave aliases,
  computed comparisons, mutation-sensitive bindings, and arbitrary predicates
  conservative until separately specified.
- [x] Diagnose a contradiction only when a reachable partition has provably
  incompatible requirements. Otherwise widen incomplete knowledge.

## 4. Define and enforce recursion boundaries

- [x] Preserve existing `recur` checking against the enclosing signature after
  its body-derived facts are solved.
- [x] Cover a wrapper that calls a previously declared function and an exported
  imported callable whose signature snapshot is available.
- [x] Add regressions showing that direct self-reference and mutually recursive
  unannotated functions do not create a non-terminating inference fixed point.
  Initially widen the cyclic constraints and retain ordinary dynamic-call
  behavior.
- [x] Do not broaden inferred-alternative preservation through structural
  function values as part of this work.

## 5. Verify frontend and module boundaries

- [x] Add module-loader coverage that imports an exported numeric function and
  proves a local wrapper receives its parameter/result precision.
- [x] Add CLI coverage for public diagnostics and a source span at the wrapper
  call that is statically incompatible.
- [x] Exercise REPL/server compilation if its retained session snapshots use a
  distinct semantic-entry path.
- [x] Run the narrow semantic, CLI, and module-loader tests while iterating;
  run `make check` before completion.

## 6. Complete the language record when implementation lands

- [x] Update the support inventory and regenerate the language support matrix.
- [x] Update the README capability statement and `changelog.md` under
  `Unreleased` to state the implemented boundary.
- [x] Keep the language specification and this ADR aligned with the final
  handling of cycles and dynamic calls. Amend the implementation plan rather
  than silently broadening the rule.

## Explicit non-goals

- Whole-program or caller-history inference.
- General fixed-point inference for mutually recursive unannotated functions.
- Constraints from dynamic/structural calls or arbitrary callable values.
- Runtime type checks, coercion, public constrained-function syntax, or
  call-boundary specialization.
