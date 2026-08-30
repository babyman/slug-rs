# Changelog

## Unreleased

- Split the CLI and VM integration-test harnesses into feature-oriented modules
  with compact entry-point maps, preserving their existing test coverage.

- Aligned contributor test routing and language-handoff documentation with the
  versioned `tests/conformance` fixture suite, and made documentation checks
  verify its mandatory repository paths.

- Added an AI-assisted development plan covering repository guidance accuracy,
  bounded working sets, crate-boundary clarity, and reproducible tooling.

- VM dispatch now retains `SpanId`s and resolves source-table entries only at
  diagnostic or durable ownership boundaries; benchmark output also reports
  runtime layout sizes and source-table lookup counts.

- The normal test targets now enable opt-in VM metrics; focused tests cover
  source-span ownership for diagnostic errors, suspended selects, spawned
  tasks, and native failures.

- Added an outstanding numeric-representation decision plan comparing the
  current checked integer/binary-float model with DEC64, IEEE decimal,
  fixed-point, runtime-specialized, arbitrary-decimal, and rational models,
  including semantic gates and VM-focused performance experiments.

- Expanded executable-layout accounting to include program, chunk, constant,
  descriptor, metadata, and source-table capacities, and added separately
  reported private-bytecode verification time to the opt-in VM metrics.

- Added opt-in scheduler scaling counters for queue scans, waiter-removal
  work, queue peaks, and blocked scheduler time, plus scaled timer benchmarks
  and a cancellation workload that proves it does not wait for its timers.

- Removed the obsolete unreachable owned VM dispatch body and its duplicate
  owned-span helpers; borrowed dispatch is now the sole opcode interpreter.

- Private-bytecode verification now rejects reachable chunk fallthrough,
  overflowing call and map operand counts, unmatched scopes, and scope-depth
  conflicts before any frame executes; malformed-opcode coverage was expanded.

- VM executions now share one installed immutable program across root closures,
  spawned tasks, and explicit nurseries. New `Rc<Program>` entry points avoid
  the compatibility wrapper's single root installation copy, and opt-in
  metrics and benchmark output report whole-program clones and estimated
  copied instruction bytes.

- The VM optimization plan now requires a dated, reproducible before/after
  measurement record for every completed future-work item.

- Audited the VM optimization plan against its implementation, clarified the
  provisional scheduler and compact-encoding decisions, and added prioritized
  follow-up work for program ownership, verification, dispatch cleanup,
  measurement, executable accounting, metrics coverage, hot-path data
  locality, and the register-VM comparison gate.

- Reorganized documentation into language, reference, engineering, planning,
  decisions, and generated areas; normalized supplemental language-document
  filenames and retained completed plans under `docs/planning/completed`.

- Expanded the private VM optimization plan with staged measurement, bytecode
  verification, capture-aware local storage, and scheduler-scaling gates.

- Private-bytecode verification now rejects `TryMatch` declarations whose
  binding count disagrees with their pattern metadata before execution.

- Sequenced the remaining borrowed-dispatch work around variable-shape calls,
  durable suspended state, and an opcode ownership audit before verifier work.

- Added a dependency-free VM benchmark harness and run-scoped dispatch,
  instruction-clone, frame, and local-binding-cell counters.

- Documented that future execution-path VM metrics must be default-disabled
  and compile to no-ops in ordinary builds.

- Added the default-disabled `metrics` feature for VM execution counters;
  `make bench-vm` enables it explicitly.

- VM dispatch now borrows private instructions rather than cloning every
  instruction before execution.

- Common VM stack, arithmetic, comparison, and control-flow operations now
  borrow instruction source spans until an error needs an owned diagnostic.

- VM list/map construction and indexed access now also borrow instruction
  source spans during successful execution.

- Tail-position `recur` now borrows source spans through argument binding and
  preserves its existing cleanup and captured-local behavior.

- VM scope exit and return handling now borrow source spans while preserving
  cleanup-driven completion behavior.

- Global declarations, closure construction, and ordinary calls now borrow
  source spans, cloning only call sites retained in runtime call frames.

- Spread, selected-overload, and pipeline calls now borrow spans through
  argument binding and native invocation, retaining ownership only for durable
  closure call frames or errors.

- Imports, list spreads, and deferred-action registration now borrow source
  spans during successful execution.

- `select` now borrows spans for ready/default execution and clones only when
  storing a suspended wait set for later scheduler diagnostics.

- Task spawning and nursery setup now borrow source spans until task-body
  execution frames are created.

- Struct, slice, and match VM operations now borrow source spans; `throw`
  retains one only for its durable runtime error.

- Completed borrowed-span VM dispatch for interpolation, captures, global and
  module metadata operations, overload setup, and `select` handler execution.

- Added initial private-bytecode verification before VM frame creation for
  local slots, jumps, function references, selected-call identities, and
  module-tag metadata.

- The private-bytecode verifier now also rejects missing constants and empty
  `select` instructions before execution.

- Added conservative private-bytecode stack-state verification for every
  chunk, including explicit `TryMatch` binding/result temporaries and
  called-frame operand prefixes.

- Private bytecode now stores compact span IDs backed by program-owned,
  source-path-interned metadata while preserving source-located diagnostics.

- Installed private bytecode now pools global names, match patterns, capture
  lists, schema fields, and struct field lists behind checked metadata IDs.

- VM dispatch now borrows pooled opcode metadata directly, avoiding transitional
  opcode reconstruction on the execution path.

- VM frames now store ordinary locals directly and create shared binding cells
  only when closures capture them; source compilation emits captures lazily.

- Added opt-in scheduler pressure benchmarks and counters for timer lifecycle
  and wait-registration cleanup; retained the existing vector/FIFO queues.

- Added executable-layout limit reporting and selected fixed-width fields for
  a future compact private encoding without creating a serialized opcode format.

- The VM benchmark now reports bytecode layout and source-map compression
  estimates to support future representation decisions.

- Private-bytecode verification now recursively validates match-pattern and
  match-type operand references before execution.

- Known struct fields now retain their declared types when bound by match and
  declaration destructuring patterns.

- `struct<S>` annotations now require `S` to resolve to a schema and retain
  stable schema identity through aliases, imports, and shadowing.

- Known local, aliased, and imported schemas now retain field metadata for
  optional checking. Construction, copies, and direct field access diagnose
  provable field-name, required-field, and field-value mistakes.

- Closed, type-constrained match subjects now receive optional diagnostics for
  uncovered members, disjoint cases, and unreachable unguarded cases. Guards
  and structural patterns remain conservative.

- Direct `== nil` and `!= nil` conditions now narrow nilable bindings within
  `if` branches and evaluated `&&`/`||` right-hand operands under
  `-type-check`, without leaking facts to the enclosing scope.

- Optional `-type-check` now validates statically known operator, index, and
  slice operands. It retains precise list/map access and list-combination
  result types while leaving unknown and `any` operands dynamic.

- Added the non-parameterized `schema` type for struct-schema values. Direct
  construction through a known schema binding `S` now infers `struct<S>`;
  `_: schema` distinguishes schema values from `_: struct` instances.

- Added whole-case match type constraints, including schema identity via
  `struct<User>`, recursive list/map checks, and case-local narrowing under
  `-type-check`. The former `Schema {field}` source pattern is replaced by
  `{field}: struct<Schema>`.

- Structural function values selected by expressions such as `if` now support
  precise positional call-result inference and optional arity/type checking.
  Named and spread function-value calls remain dynamic because structural
  function types do not carry call-label or variadic metadata.

- Function expressions and declared foreign functions now retain structural
  `fn<result, parameter...>` value types. Inferred function results propagate
  through direct bindings and calls, enabling precise optional checks for
  function-valued declarations and collections.

- Completed overload conformance coverage for both compiler modes, union and
  nilable specificity, named/defaulted/variadic calls, explicit generic
  applications, ambiguity, selected pipelines, and live replacement. Pipeline
  member calls now participate in static overload resolution.

- Defined spread-call resolution for static overload sets: calls with a
  `...spread` argument now fail semantically instead of selecting an overload
  from unknown runtime arity. Singleton known callables retain normal spread
  binding.

- Resolved `foreign` bindings now carry their private canonical callable
  identities. Statically selected foreign-only and mixed local/foreign
  overloads dispatch to the declared live member without native type checks.

- Local function declarations with the same name now form checked overload
  sets in module and nested lexical scopes. Distinct canonical signatures keep
  all live closures, duplicate signatures are semantic errors, and exported
  overloads remain one live binding.

- Defined `any` as the top type for non-nil values and `any|nil` as the
  universal value type. Unannotated parameters canonicalize to `any|nil`, while
  omitted binding and result annotations infer a precise type before falling
  back to `any|nil`; the checker's unknown state remains private.
- Added the canonical semantic type foundation and mandatory annotation
  resolution in both compiler modes. Unknown type names and invalid type
  constructor arity are checked source errors; optional checking now enforces
  `any` nilability, normalized unions, structured reflexivity, and non-nil bare
  generic inference.
- Added lexically scoped semantic bindings with ordered callable sets. Locally
  known calls now perform mandatory call-shape binding, generic inference, and
  parameter-type resolution in both compiler modes; nested declarations and
  parameters correctly shadow outer callable metadata, and immutable aliases
  retain it.
- Added cached semantic snapshots for exported callable sets. Loader-backed
  compilation installs imported signatures through module member access,
  explicit map destructuring, and `{*}` selection while preserving generic,
  nilability, default, and variadic metadata. This exposed and fixed
  `slug.channel.send` and `trySend` return annotations to retain `chan<any|nil>`.
- Lowered statically selected overload identities into private call and
  pipeline-call bytecode. Source callable closures retain their canonical
  input identities across module merging, and the VM dispatches the selected
  member from the current live binding without runtime type validation. A live
  binding that no longer contains the selected identity now fails with a
  checked call error instead of silently choosing another overload.
- Made lower generic arity the specificity tie-breaker when applicable
  overloads have equivalent instantiated parameter types. Concrete overloads
  now take priority over generic fallbacks independently of import order,
  while equally generic candidates remain ambiguous.

- Defined static overload signatures by their type parameters and inputs.
  Parameter annotations participate in mandatory overload resolution in every
  compiler mode, while `-type-check` adds diagnostics without changing
  selection. Return annotations remain available for checking and inference
  but do not participate in overload identity or selection. Signature equality
  uses canonical input structure, including alpha-normalized generics and
  normalized unions, rather than source spelling or assignability.

- Removed the ambient `send` and `recv` builtins. Channel operations are now
  available exclusively through the `slug.channel` source-library bindings,
  backed by `select` send and receive case forms.

- Removed the ambient `await` builtin. Task joining is now provided by the
  `slug.channel.await` source-library wrapper over the `select { await task }`
  case form.

- Removed the ambient `close` builtin. Channel closure is available exclusively
  through the exported `slug.channel.close` library binding.

- Added the implicit `slug.builtin` foundation module. Host registrations are
  available independently of its optional source file; that file documents
  host functions and supplies the standard `Error` schema. The module remains
  explicitly importable, and local declarations retain precedence.

- Added module-qualified declared-foreign resolution and the initial
  `slug.channel` library. Its `chan` and `close` bindings use the static host
  registry; source wrappers provide send, receive, non-blocking operations,
  and timed task/channel waits.

- `SLUG_HOME` now supplies both the optional library configuration and the
  `$SLUG_HOME/lib` source-library root used by `import`.

- Added `_` discard parameters for functions; they receive positional arguments
  without creating a body-visible binding.

- Added parsing and retained metadata for documented, tagged, and exported
  `foreign` declarations. Host foreign-resolution remains a later milestone.

- Fixed concurrency owner settlement, `select` arbitration, and native wakeups:
  failed owners now settle or cancel their children, explicit nursery bodies
  may suspend, losing task-await cases no longer suppress failures, waiter
  callbacks cannot reborrow task state, and native producer notifications
  compete correctly with timer deadlines without a fixed polling window.
- Implemented bounded native channel producers with restricted owned send
  values, thread-safe mailbox publication, shared capacity accounting with
  Slug sends, receiver-drop revocation, and checked close wakeups.
- Native callbacks can create a receiver/producer pair; accepted producer
  mailbox values are converted and delivered only by the VM thread when that
  receiver is read or selected.
- Implemented `select` for receive, send, millisecond timer, task-await, and
  default cases, including optional handlers. Select suspensions now remove all
  losing channel, task, and timer registrations as soon as one case wins.
- Added regression coverage for selected task failures: they now demonstrably
  follow ordinary `throw` cleanup and `defer onerror` recovery paths.
- Completed the structured-concurrency roadmap: root and explicit nurseries,
  task ownership and limits, cancellation, settlement, spawn capture, repeated
  awaits, channels, native producer capabilities, and `select` are complete.
- Added focused VM coverage for suspended select-await resumption, losing
  channel-waiter removal, cancellation of timer/channel waiters, and checked
  malformed private select bytecode errors.
- Added bounded FIFO channel builtins: `channel(capacity)`, `send`, `recv`,
  and idempotent `close`. Spawned tasks now suspend without unwinding their VM
  state, preserve deferred cleanup while blocked, and resume through FIFO
  sender/receiver queues. Root evaluations now suspend and resume through the
  same scheduler. Cancelling a parked task now removes its pending channel or
  task-await registration. `select` remains deferred.
- Added preliminary cooperative task handles through `spawn` and `await`, plus
  explicit `nursery` and `nursery limit N` source forms, as the first
  cooperative structured-concurrency slice; unawaited child failures now
  propagate when their root evaluation settles, and task VMs share live root
  and module globals with their parent evaluation. `nursery limit 0` now
  rejects direct spawns. Nested task VMs inherit their dynamic nursery, while
  an explicit nursery creates a distinct owner. Tasks defer execution until an
  await or owner settlement, preserving the specified spawn-capture boundary.
  Explicit nurseries now logically cancel pending siblings after their first
  unobserved child failure. Nursery task limits now admit direct children up to
  capacity, queue further direct spawns, and release admission slots when tasks
  settle; awaiting a queued task preserves the nursery's direct-child admission
  order. Task execution now uses a deterministic FIFO ready queue, so awaiting
  a later ready task first drives earlier spawned siblings.
- Fixed version 0 native resource cleanup across failed close callbacks,
  structured error data, shared module-loader runtimes, and long-lived resource
  registries; native callback panics no longer emit a host panic diagnostic.
- Replaced direct Rust `Value` exposure in native callbacks with the opaque,
  call-scoped version 0 facade, including checked conversions and result
  contracts, structured error code/message/data, panic containment,
  module-and-type-checked resources, idempotent close, and runtime teardown.
- Defined the opaque native extension interface that precedes concurrency,
  including synchronous calls without scheduler hints, structured errors,
  typed resources, revocable thread-safe channel producers, explicit shutdown,
  admission-based nursery limits, and the gate for a future versioned C ABI and
  external FFI loader.
- Added a minimal metadata-backed syntax conformance suite derived from the
  legacy Go Slug sources, covering supported syntax without legacy assertions.
- Simplified the specified program entrypoint rule: only a local, top-level
  zero-argument `main` in the program module is invoked after initialization.
- Implemented automatic invocation of the program module's local,
  zero-argument `main` after successful initialization.
- Made host-native bindings, including `println`, available while imported
  modules initialize.
- Added module fixture coverage for library fallback and imported-module failure
  locations, alongside resolution, caching, cycles, and live exports.
- Added immutable configuration collection with TOML, environment, and program
  option precedence.
- Added `cfg`, `argv`, and `argm` builtins with module-relative configuration
  keys and fallback-shaped environment and command-line conversion.
- Added versioned portable conformance-fixture metadata for expected outcomes,
  streams, roots, timeouts, and exact diagnostics.
- Added the `slug-fixtures` conformance runner, which isolates fixture hosts and
  rejects missing metadata, timeouts, crashes, stream mismatches, and diagnostic
  mismatches.
- Retained top-level declaration documentation and evaluated tag arguments in
  module instances for future metadata introspection.
- Added source-callable overload sets for distinct imported signatures and
  warnings when a later module duplicates an imported callable signature.
- Added module warnings for local bindings that shadow `{*}` imports and for
  duplicate non-callable names across a multi-module import.
- Made imported functions run in their defining module and preserve live
  exported binding values across calls.
- Predeclared statically known module bindings, allowing cyclic imports while
  reporting checked errors for reads before a binding initializes.
- Implemented top-level `{*}` map selection declarations, including importing
  every string-keyed module export into the current module scope.
- Added dedicated source-level `import(name, ...)` execution with checked
  string module names, shared cached resolution and initialization, nested
  importer-relative loading, and first-module-wins exported-map results.
- Added top-level `export` declaration parsing and retained exported-name
  metadata for the module loader.
- Added the `???` placeholder, which raises a checked `not implemented` runtime error.
- Reordered the language foundation roadmap so all `slug.*` public-library
  implementation follows the language, module, runtime-service, and VM work.
- Added declaration and parameter tag syntax with source-order tag-argument
  evaluation. Retained module metadata and `slug.meta` introspection remain
  future work.
- Added strict `/** ... */` documentation blocks attached to top-level
  declarations. Module documentation and metadata introspection remain future
  work.
- Replaced the export semantics formerly associated with `@export` with the
  `export` declaration keyword; `@export` remains an ordinary tag. Also
  replaced `@main` with source-ordered discovery of a local `main` function
  eligible for a zero-argument call in the target language specification.
- Added declaration, parameter, return, and struct-field annotation syntax,
  plus opt-in static validation through `-type-check`, generic call inference,
  and explicit type applications.
- Added schema-identity struct patterns with partial named-field matching.
- Added pipeline calls and subjectless pipeline `match` expressions with `/>`.
- Added checked `string * non-negative-integer` repetition.
- Added immutable list concatenation (`+`), append (`:+`), and prepend (`+:`)
  with checked list-operand failures.
- Added checked integer bitwise, shift, and prefix `~` operators.
- Added `$identifier` string interpolation; embedded expressions and property
  access remain deliberately unsupported.
- Added schema-preserving struct copies with checked replacement fields.
- Added source-level `%` with checked zero-division behavior.
- Added one-to-three-digit octal escapes in double-quoted strings.
- Added raw and triple-quoted strings, including the specified basic escape
  behavior and checked unterminated-string diagnostics.
- Added decimal floating-point, exponent, hexadecimal, and byte source
  literals with checked malformed-literal diagnostics.
- Added checked list slicing with optional start, end, and step expressions;
  `list[:end]` now starts at zero without requiring a redundant `0`.
- Added a detailed inventory of the expression foundation and split the language
  support matrix so partial literal, operator, collection, struct, annotation,
  and metadata support is explicit.
- Made tail-position `recur(...)` share ordinary positional, named, default,
  variadic, and spread argument binding, including function match bodies.
- Added call-time default parameters evaluated in the callee's defining
  environment.
- Added final variadic parameters, including checked named rest values.
- Added named ordinary-function arguments with checked unknown and duplicate
  parameter diagnostics.
- Added positional call spreads and list-literal spreads with left-to-right,
  single-evaluation behavior and checked non-list failures.
- Added a dependency-ordered language foundation roadmap covering complete call
  semantics, remaining expressions, modules, conformance, concurrency, and the
  gate for measured VM optimization.
- Documented the measured VM optimization plan, compact metadata direction,
  capture-aware local storage, and the decision gate for any future register
  VM.
- Added identity-bearing untyped struct schemas, stored field defaults, checked
  construction, structural instance equality, and field access.
- Added bracketed computed map-pattern keys, evaluated once through indexed
  runtime pattern operands.
- Added pinned `^name` patterns backed by indexed runtime pattern operands.
- Added comma-separated, non-binding alternatives within a `match` case.
- Added `name @ pattern` matching and destructuring to bind a whole value
  alongside its nested pattern bindings.
- Added anonymous final `...` patterns that discard remaining list items or
  map entries.
- Fixed deferred cleanup to preserve caller scopes, run older cleanup after a
  replacement error, and drain a deferred action's own cleanup before return.
- Fixed successful non-tail `match` stack cleanup and nested-scope cleanup
  before `recur(...)` starts its next iteration.

- Added `defer onerror(err)` cleanup, structured VM-fault bindings, and
  recovery that returns the handler result from the handling function.

- Adopted `{type, msg, data}` as the Slug-visible VM-fault value for future
  `defer onerror` handlers.

- Added `defer onsuccess` cleanup for normal scope completion.

- Extended plain `defer` cleanup to checked VM runtime faults.

- Added LIFO plain `defer` cleanup for normal returns and uncaught throws.

- Added checked language-level `throw` with the thrown Slug value, source
  location, and call frames retained by uncaught runtime errors.

- Clarified the policy requiring undocumented Slug-visible decisions to be
  recorded in their owning normative document before implementation.

- Added exact map patterns with `{| ... |}`.
- Added named `...rest` captures for non-exact map patterns.
- Added list and map destructuring for `val` and `var` declarations.
- Added function match bodies with their parameter-derived subjects.
- Added non-exact string-key map patterns for `match`.
- Added `if` guards for match cases.
- Added literal and list-pattern `match` expressions with case-local bindings.
- Preserved escaping closure captures across `recur(...)` iterations.
- Added stack-safe tail recursion through `recur(...)`.
- Clarified that Slug uses recursion, including tail-position `recur(...)`, for
  repetition and has no `while`, `for`, `loop`, `break`, or `continue` forms.
- Added explicit `return expression` for early exit from source functions.
- Removed symbols from the language value model. Bare map keys, map patterns,
  dot access, module exports, and configuration keys now use strings.
- Preserved integer precision in integer arithmetic and comparisons; rejected
  oversized bytecode calls without host overflow; and improved map dot lookup
  plus runtime frame names and call-site spans.
- Added parser nesting limits and support for `//`, `/* ... */`, and `/** ... */`
  comments.
- Fixed parser stack overflow risks for long prefix sequences and corrected
  comments, infix continuation, delimited multiline expressions, and brace
  disambiguation at source newlines.
- Added short-circuit `&&` and `||` expressions with operator-aware newline
  continuation.
- Added shared mutable binding cells so closures and sibling closures observe
  assignments to captured `var` bindings.
- Added a span-aware Rust source frontend for lexical bindings, functions and
  captures, blocks, conditionals, comparisons, and list/map indexing.
- Added source-located parse and semantic diagnostics plus source spans on
  emitted runtime instructions.
- Adopted a portable `.cslug` compiled-module compatibility contract and
  documented the required versioning, validation, and implementation gate.
- Added the Rust Slug bytecode VM foundation.
- Added initial source parsing, bytecode compilation, and CLI execution for
  bindings, assignments, literals, arithmetic, calls, comments, and `println`.
- Added Codex-focused repository guidance, language-change workflows, local
  validation targets, and continuous integration for agentic development.
- Moved language documentation under `docs/`, added scoped documentation
  guidance, and added an automatically checked language support matrix.
