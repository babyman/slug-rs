# Changelog

## Unreleased

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
