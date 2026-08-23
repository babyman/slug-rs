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
Public `slug.*` library modules are deliberately deferred until the language,
module system, runtime services, and VM behavior they expose are complete.

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
- [x] Evaluate arguments and spreads exactly once in source order.
- [x] Centralize parameter binding for ordinary calls and `recur(...)` so both
  enforce the same positional, named, default, and variadic rules.
- [x] Evaluate omitted default expressions at call time in the function's
  defining module environment.
- [x] Expand list-valued call spreads and list-literal spreads while preserving
  left-to-right evaluation order.
- [x] Preserve function-match-body subjects after default and variadic binding.
- [x] Preserve checked unwinding, source spans, and Slug call frames for call
  binding failures.

### Required coverage

- [x] Cover ordinary, named, defaulted, variadic, and mixed spread calls through
  public source tests.
- [x] Cover multiple spreads and side-effecting argument expressions to prove
  evaluation order and single evaluation.
- [x] Cover non-list spreads, unknown and duplicate names, missing required
  arguments, excessive positional arguments, duplicate parameter names, and an
  invalid named value for a variadic parameter.
- [x] Cover closures in default expressions and the defining-module environment
  rule.
- [x] Cover `recur(...)` and function match bodies with defaulted and variadic
  parameters.
- [x] Add focused VM tests for any new bytecode validation or runtime boundary.
- [x] Synchronize the grammar, specifications, support manifest, generated
  matrix, README, and changelog; create a decision record only if implementation
  work introduces a new non-trivial language or runtime-architecture decision.
- [x] Run `make check`.

## 2. Finish the non-module expression foundation

- [x] Inventory the remaining grammar against the source AST, parser, compiler,
  VM, and tests; split the coarse support-matrix rows where partial support is
  currently hidden.
- [x] Implement the remaining specified numeric, byte, and string literal forms,
  including interpolation.
- [x] Implement the remaining arithmetic, bitwise, shift, list-concatenation,
  and pipeline operators with checked type failures.
- [x] Implement list slicing and finish collection access behavior.
- [x] Implement struct copy.
- [x] Implement struct patterns.
- [x] Implement parameter, return, declaration, and struct-field type annotation
  syntax and its required static checks.
- [x] Implement tags, documentation statements, and the `???` form in
  dependency order.
  - [x] Parse declaration/parameter tags and evaluate their arguments.
  - [x] Parse and attach strict documentation blocks to top-level declarations.
  - [x] Implement the `???` form as a checked runtime placeholder.
- [x] Run `make check` after each independently supported feature slice.

## 3. Add modules

Follow the module rules in
[Modules, imports, and exports](language/language-specification.md#modules-imports-and-exports)
and the host boundary in
[Runtime Requirements](language/runtime-requirements.md#required-host-services).

- [x] Define a source loader and explicit module-root and library-root host
  services without exposing host capabilities as ordinary Slug bindings.
- [x] Implement `import(name)` resolution, module caching, and checked module
  diagnostics.
  - [x] Cache compiled modules by resolved source path.
  - [x] Define isolated cached module-instance execution for imported values.
  - [x] Resolve and execute source-level `import(name, ...)` calls.
- [x] Implement top-level `export` declarations and module export maps.
  - [x] Parse top-level `export` declarations and retain exported names.
  - [x] Construct module export maps from initialized bindings.
- [x] Predeclare statically knowable top-level bindings and support cyclic module
  initialization with checked use-before-initialization failures.
- [x] Implement live imported bindings.
- [x] Implement imported-name shadowing and non-callable import conflict
  warnings.
- [x] Implement callable import conflict and combination rules.
- [x] Retain declaration, tag, and documentation metadata in the module model
  for later introspection.
- [x] Invoke the main program module's local zero-argument `main` function
  after successful top-level evaluation.
- [x] Add module fixtures for relative resolution, library fallback, caching,
  cycles, live exports, and failure locations.
- [ ] Run `make check`.

## 4. Add configuration and the conformance harness

- [x] Implement the immutable configuration store and precedence rules from
  [`language/configuration.md`](language/configuration.md).
- [x] Implement `cfg`, `argv`, and `argm` with module-relative namespaces and
  checked conversions.
- [x] Add portable fixture metadata for outcome, streams, roots, timeout, and
  optional exact diagnostics.
- [x] Build a runner that rejects unclassified fixtures and treats every host
  panic as a conformance failure.
- [x] Prove the runner with library-independent fixtures, including exit status,
  standard output, standard error, diagnostic category, and source location.
- [ ] Run `make check`.

## 5. Establish the native extension boundary

- [ ] Replace the current Rust `NativeFunction` value exposure with the opaque,
  call-scoped version 0 facade from [`native-abi.md`](native-abi.md).
- [ ] Add checked argument, result, structured-error, and native-resource
  operations without persistent roots or scheduler hints.
- [ ] Prove wrong-type, wrong-resource, callback-contract, panic-containment,
  and teardown behavior in focused VM tests.
- [ ] Keep the Rust facade explicitly unstable until concurrency validates it;
  do not add dynamic loading or publish a C ABI yet.
- [ ] Run `make check`.

## 6. Add structured concurrency

- [ ] Implement the implicit root task owner and explicit nurseries.
- [ ] Implement task handles, spawn capture, ownership, limits, cancellation,
  settlement, and repeated await behavior.
- [ ] Implement channel values, their checked runtime operations, and the
  bounded native producer capability defined by the native interface.
- [ ] Implement `select` cases for receive, send, timer, await, and default.
- [ ] Integrate task failure with `throw`, deferred cleanup, and checked runtime
  diagnostics.
- [ ] Implement the timer host service used by timer-oriented language forms.
- [ ] Add focused runtime and VM coverage for concurrency behavior.
- [ ] Run `make check`.

## 7. Add the public library and run full conformance

Implement the public library only after its language and runtime foundations
are stable:

- [ ] Implement `slug.meta` introspection over retained module, declaration,
  tag, and documentation metadata.
- [ ] Implement minimal `foreign` declarations and a declared-foreign host
  registry, keeping FFI and ABI adaptation outside the language runtime.
- [ ] Implement `slug.test` assertions and fixture support.
- [ ] Implement the required `slug.std` core and collection operations.
- [ ] Implement the `slug.channel` API over the completed channel and task
  runtime.
- [ ] Implement the `slug.time` API over the completed timer host service.
- [ ] Run all non-concurrent supported and error-parity fixtures.
- [ ] Run the remaining concurrency fixtures.
- [ ] Verify exit status, standard output, standard error, diagnostic category,
  and source location where fixture metadata makes them exact.
- [ ] Run `make check`.

## 8. Stabilize the native ABI and add external FFI

After channels and concurrency have exercised the native boundary:

- [ ] Stress resource cleanup, close races, cross-thread sends, cancellation,
  producer revocation, and runtime teardown.
- [ ] Publish the version 1 C declarations, version negotiation, loader
  validation, and ABI conformance tests together.
- [ ] Add dynamic Slug-aware module loading only after version 1 is fixed.
- [ ] Specify any TOML raw C bridge independently of the native module ABI.

## 9. Optimize only from measurements

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
