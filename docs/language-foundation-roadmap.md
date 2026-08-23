# Language Foundation Roadmap

This document tracks implementation work needed to move the Rust VM from its
current core subset toward clean-room source compatibility. It is a task list,
not a source-language specification. The documents under [`language/`](language/README.md)
remain authoritative for syntax and observable behavior, and the generated
[language support matrix](generated/language-support.md) remains the status
summary.

Complete the milestones in dependency order. A milestone is complete only when
its accepted source forms, checked failures, implementation tests, language
records, support matrix, README capability statement, and changelog entry agree.

## 1. Complete function-call semantics

The first priority is the call boundary described by the
[Functions and calls](language/language-specification.md#functions-and-calls)
section and the
[Variadic Functions and Spread Syntax](language/Variadic%20Functions%20and%20Spread%20Syntax%20-%20Mini%20Spec.md)
note.

### Representation and parsing

- [x] Replace name-only function parameters in the source AST with parameter
  metadata for names, defaults, and a final variadic parameter.
- [x] Represent positional, named, and spread call arguments distinctly in the
  source AST.
- [x] Parse default parameters, final `...rest` parameters, named arguments,
  call spreads, and list-literal spreads.
- [x] Reject malformed parameter lists and positional arguments appearing after
  a named argument with source-located diagnostics.

### Compilation and execution

- [x] Add private bytecode metadata sufficient to describe callable signatures
  without making that representation a portable compatibility promise.
- [ ] Evaluate arguments and spreads exactly once in source order.
- [ ] Centralize parameter binding for ordinary calls and `recur(...)` so both
  enforce the same positional, named, default, and variadic rules.
- [x] Evaluate omitted default expressions at call time in the function's
  defining module environment.
- [x] Expand list-valued call spreads and list-literal spreads while preserving
  left-to-right evaluation order.
- [x] Preserve function-match-body subjects after default and variadic binding.
- [ ] Preserve checked unwinding, source spans, and Slug call frames for call
  binding failures.

### Required coverage

- [ ] Cover ordinary, named, defaulted, variadic, and mixed spread calls through
  public source tests.
- [ ] Cover multiple spreads and side-effecting argument expressions to prove
  evaluation order and single evaluation.
- [ ] Cover non-list spreads, unknown and duplicate names, missing required
  arguments, excessive positional arguments, duplicate parameter names, and an
  invalid named value for a variadic parameter.
- [ ] Cover closures in default expressions and the defining-module environment
  rule.
- [ ] Cover `recur(...)` and function match bodies with defaulted and variadic
  parameters.
- [ ] Add focused VM tests for any new bytecode validation or runtime boundary.
- [ ] Synchronize the grammar, specifications, support manifest, generated
  matrix, README, and changelog; create a decision record only if implementation
  work introduces a new non-trivial language or runtime-architecture decision.
- [ ] Run `make check`.

## 2. Finish the non-module expression foundation

- [ ] Inventory the remaining grammar against the source AST, parser, compiler,
  VM, and tests; split the coarse support-matrix rows where partial support is
  currently hidden.
- [ ] Implement the remaining specified numeric, byte, and string literal forms,
  including interpolation.
- [ ] Implement the remaining arithmetic, bitwise, shift, list-concatenation,
  and pipeline operators with checked type failures.
- [ ] Implement list slicing and finish collection access behavior.
- [ ] Implement struct copy and struct patterns.
- [ ] Implement parameter, return, declaration, and struct-field type annotation
  syntax and its required static checks.
- [ ] Implement tags, documentation statements, foreign declarations, and the
  `???` form in dependency order.
- [ ] Run `make check` after each independently supported feature slice.

## 3. Add modules and the initial standard library

Follow the module rules in
[Modules, imports, and exports](language/language-specification.md#modules-imports-and-exports)
and the host boundary in
[Runtime Requirements](language/runtime-requirements.md#required-host-services).

- [ ] Define a source loader and explicit module-root and library-root host
  services without exposing host capabilities as ordinary Slug bindings.
- [ ] Implement `import(name)` resolution, module caching, and checked module
  diagnostics.
- [ ] Implement top-level `@export` discovery and module export maps.
- [ ] Predeclare statically knowable top-level bindings and support cyclic module
  initialization with checked use-before-initialization failures.
- [ ] Implement live imported bindings, shadowing behavior, import conflicts,
  and callable combination rules.
- [ ] Invoke the unique local `@main` entrypoint after successful top-level
  evaluation.
- [ ] Add the minimum `slug.test` and `slug.std` surface needed to execute the
  first non-concurrent conformance fixtures.
- [ ] Add module fixtures for relative resolution, library fallback, caching,
  cycles, live exports, and failure locations.
- [ ] Run `make check`.

## 4. Add configuration and the non-concurrent conformance runner

- [ ] Implement the immutable configuration store and precedence rules from
  [`language/configuration.md`](language/configuration.md).
- [ ] Implement `cfg`, `argv`, and `argm` with module-relative namespaces and
  checked conversions.
- [ ] Add portable fixture metadata for outcome, streams, roots, timeout, and
  optional exact diagnostics.
- [ ] Build a runner that rejects unclassified fixtures and treats every host
  panic as a conformance failure.
- [ ] Run all non-concurrent supported and error-parity fixtures.
- [ ] Verify exit status, standard output, standard error, diagnostic category,
  and source location where fixture metadata makes them exact.
- [ ] Run `make check`.

## 5. Add structured concurrency

- [ ] Implement the implicit root task owner and explicit nurseries.
- [ ] Implement task handles, spawn capture, ownership, limits, cancellation,
  settlement, and repeated await behavior.
- [ ] Implement channels and the `slug.channel` public surface.
- [ ] Implement `select` cases for receive, send, timer, await, and default.
- [ ] Integrate task failure with `throw`, deferred cleanup, and checked runtime
  diagnostics.
- [ ] Implement the timer host service and the required `slug.time` surface.
- [ ] Run the remaining concurrency fixtures and `make check`.

## 6. Optimize only from measurements

After the source foundation and representative workloads exist, follow the
separate [VM Optimization Plan](vm-optimization.md).

- [ ] Establish the Stage 0 benchmark and instrumentation baseline.
- [ ] Optimize dispatch, metadata, and local storage one measured stage at a
  time.
- [ ] Keep private bytecode optimization separate from the portable `.cslug`
  format.
- [ ] Re-run conformance and checked-failure coverage after every representation
  change.

Portable `.cslug` encoding and loading remain a later compatibility milestone.
Do not serialize the private `Program`, `Chunk`, or `Op` representation as a
shortcut.
