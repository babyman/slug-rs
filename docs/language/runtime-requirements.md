# Slug Runtime Requirements

## Status and purpose

This document specifies the observable runtime obligations of a conforming
Slug implementation. It is written for a clean-room implementation: a
developer must be able to implement the language from this document, the
[Language Specification](language-specification.md), the public library
sources, and source-level conformance fixtures without reading or copying an
existing implementation.

These requirements deliberately do **not** standardize private VM bytecode,
opcode numbers, instruction layout, stack layout, garbage collection, host
threading, or a foreign-function ABI. Those are implementation choices. The
private bytecode must not be serialized or treated as a cross-version
interface. The separate `.cslug` compiled-module contract is specified in
[`../compiled-artifacts.md`](../compiled-artifacts.md); it is not a promise
about an implementation's private bytecode.

The requirements cover evaluation, host services, errors and cleanup, recursion,
and structured concurrency. An implementation may use an interpreter, bytecode
VM, JIT, or another execution strategy.

The keywords **MUST**, **MUST NOT**, **SHOULD**, and **MAY** in this document
describe conformance requirements. A requirement is observable when it changes
a Slug value, error, stream, module binding, task outcome, or the completion of
a conformance fixture.

## Clean-room conformance target

A conforming implementation MUST execute Slug source files and the public
library sources. It MUST NOT require Go ASTs, Go object representations, Go
goroutines, or the current bytecode. The source-level conformance target is:

- `../../tests/vm-conformance/supported`: each top-level fixture MUST complete without
  a parse, semantic, or runtime error. Assertions in `slug.test` define the
  expected values.
- `../../tests/vm-conformance/error-parity`: each top-level fixture MUST fail. The
  failure MAY occur during semantic analysis or evaluation when the language
  rule permits either, but it MUST be a Slug diagnostic rather than a host
  crash.
- nested `mod/` directories are module fixtures, not independently executed
  programs.
- `../../lib/slug`: source implementations of required standard-library modules.

The current repository harness partitions every fixture into execution and
runtime-boundary sets. A clean-room runner SHOULD run every top-level fixture
in both directories and fail if an unclassified fixture is added. The current
fixtures use source-level assertions and successful process completion as their
primary oracle. `stdout-output.slug` and `stderr-output.slug` additionally
demonstrate the required output streams, although exact stream matching should
be added to the portable conformance runner before treating it as a release
gate.

### Fixture environment

For each fixture, the runner MUST set the fixture directory as the project
module root and the repository's `lib` directory as the standard-library root.
A dot-separated import name `a.b` resolves to `a/b.slug`. The importing
directory is searched before the standard-library root. The fixture receives no
implicit host bindings beyond the documented builtins, standard-library modules,
and optional command-line arguments.

The reference command-line shape is:

```text
slug [-root MODULE_ROOT] program.slug [arguments...]
```

Successful execution exits with status zero. Parse, semantic, module-loading,
or runtime failure exits nonzero and writes a Slug diagnostic to standard error.
The final expression value is not implicitly printed by the command-line
runner. `println` writes to standard output and `slug.io.stderr.println` writes
to standard error.

### Portable fixture metadata

Source files alone can assert ordinary successful behavior through `slug.test`,
but they cannot fully describe expected process streams, time limits, or an
expected top-level failure. A portable clean-room conformance release MUST
therefore provide an adjacent manifest or sidecar for every entry fixture. The
sidecar is part of the conformance suite, not part of the language grammar.

Each fixture record MUST identify:

```text
fixture: relative/path/to/program.slug
suite: supported | error-parity
module_root: relative/path
library_root: relative/path/to/lib
timeout_ms: positive integer
expect:
  outcome: success | parse_error | semantic_error | module_error | runtime_error
  stdout: exact text | unspecified
  stderr: exact text | unspecified
  error:
    message: exact text | unspecified
    source: path, line, column | unspecified
```

`success` requires exit status zero. Every error outcome requires a nonzero
exit status and a Slug diagnostic. Any field marked `exact` is byte-for-byte
observable, including final newlines. A field marked `unspecified` is not a
portable compatibility promise. The manifest MAY add an expected serialized
result only after Slug defines a stable value serialization.

The current repository fixture directories predate this manifest. They are a
valuable source suite, but their success/error directory placement and in-file
assertions are the only portable expectations currently encoded. Implementers
must not infer undocumented stream text or exact error wording from a host
implementation.

### Required fixture library surface

For the current source fixtures, a conformance environment MUST make these
modules and builtins available with the behavior exercised by their source:

- builtins including `import`, `len`, `print`, and `println`;
- `slug.std` and `slug.test` for assertions and core collection operations;
- `slug.channel` for channels, `await`, `send`, `recv`, and `close`;
- `slug.time` for timer-oriented fixtures;
- `slug.io.stderr` for standard-error output.

The normative signatures and behavior belong to the source modules in
`../../lib/slug` and their library-reference pages. A clean-room implementation may
write them in another implementation language, but it MUST expose the same
Slug-visible module names, exports, results, errors, and stream behavior.

### Error observability

Every source-originated failure MUST identify a category, source path, line,
and column when the location is available. The portable categories are parse,
semantic, module, and runtime error. A runtime error MUST retain its Slug
payload and a Slug frame trace. Implementations MAY format diagnostics
differently unless fixture metadata marks text as exact. They MUST NOT expose a
host exception, stack trace, or panic as the Slug diagnostic.

## Conformance evidence

The source fixtures are the portable acceptance surface. The repository also
contains implementation tests, which may clarify a rule but are not a required
dependency for a clean-room implementation. The [VM throughput rewrite plan](https://github.com/sluglang/slug/blob/master/docs/planned/vm-throughput-rewrite.md)
records implementation history and is non-normative where it conflicts with
this document or the Language Specification.

The [VM throughput rewrite plan](https://github.com/sluglang/slug/blob/master/docs/planned/vm-throughput-rewrite.md)
records the more detailed semantic contract and planned implementation work.
An implementation must not trade away a requirement in this document for an
optimization.

## Required host services

The evaluator operates in a module environment and needs a small host-services
boundary. A conforming host MUST provide:

- source loading and a stable path for diagnostics;
- module loading using the fixture-environment rules above;
- standard output and standard error streams;
- monotonic timer support for `after` select cases;
- a configurable default nursery limit;
- the immutable configuration service defined in [Configuration](configuration.md);
- a registry for declared foreign functions, when a program declares them.

The evaluator MUST keep these host capabilities separate from Slug bindings.
Host services cannot become names visible to a Slug program except through a
documented builtin, imported library module, or declared foreign function.

Foreign functions are not required for the portable fixture set unless a
fixture imports a library that declares one. A declared function resolves by
its module name and local declaration name, not by an ambient host-global name.
If a required foreign function is unavailable, the implementation MUST report
a Slug module or foreign-resolution error rather than substitute host behavior
silently.

## Configuration service

Before evaluating any program or imported module, a conforming runtime MUST
construct one immutable configuration store. It MUST expose that store through
the `cfg(key, default)` builtin and MUST NOT let later changes to its source
files, environment, or command line mutate values visible to a running Slug
program.

The reference configuration sources, from lowest to highest precedence, are:

1. `$SLUG_HOME/lib/slug.toml`, when `SLUG_HOME` is set;
2. `slug.toml` in the selected module root, which replaces colliding library
   keys;
3. environment variables beginning with `SLUG__`;
4. command-line options after the entry program name.

TOML tables flatten into dot-separated keys. For example,
`[slug.web.server] port = 3000` defines `slug.web.server.port`. Strip
`SLUG__` from an environment-variable name and replace every `__` with `.`:
`SLUG__slug__web__server__port=3001` defines the same key. Missing or malformed
configuration files currently contribute no values; a conforming implementation
MUST NOT expose host parser failures to Slug code merely because an optional
configuration file is absent or malformed.

The command-line option grammar is `--key=value`, `--key value`, `-k value`, or
a valueless boolean flag. Repeated options form a list. A command-line key
without a dot is prefixed with the entry module name, so `slug server.slug
--port 3002` defines `server.port`; dotted keys are absolute. `argv()` exposes
the original post-program arguments, while `argm()` exposes their parsed option
and positional forms.

`cfg` requires exactly two arguments. Its first argument MUST be a string. A
dotted key is looked up as written. A non-dotted key is prefixed with the
current module's fully-qualified name, making `cfg("port", 8080)` in an
imported module independent of its importer's configuration. If no key exists,
`cfg` returns the supplied fallback. TOML values preserve their scalar or list
shape. Environment and command-line values begin as strings and are converted
to a number or boolean when the fallback has that type; with a list fallback a
single string becomes a one-element list. If conversion is unavailable, the
string remains a string.

## Clean-room implementation checklist

An independent implementation can reach useful conformance in the following
order without adopting any reference-internal representation:

1. Parse and evaluate literals, lexical bindings, functions, calls, control
   flow, lists, maps, structs, and patterns from the Language Specification.
2. Implement source-order evaluation, language diagnostics, scope cleanup, and
   `recur` before adding task execution.
3. Load source modules and `../../lib/slug`, including exports, live imports, cyclic
   initialization, and the `slug.test` assertion library.
4. Run the non-concurrent fixtures in `../../tests/vm-conformance/supported` and the
   error fixtures, treating every host crash as a conformance failure.
5. Implement channels, task handles, nurseries, spawn capture, await, timers,
   and `select`, then run the remaining fixtures.
6. Verify standard output, standard error, exit status, and diagnostic source
   location in addition to assertion results.

A passing fixture suite is necessary but not sufficient: the implementation
MUST also meet every observable rule in this document when a fixture does not
yet cover it.

## Program and function evaluation

For a program module, the runtime MUST perform these phases in order:

1. load and parse the source;
2. perform required semantic validation;
3. create the module environment and predeclare statically knowable top-level
   bindings for cyclic imports;
4. evaluate top-level statements in source order;
5. if top-level evaluation succeeds, invoke the program module's local,
   top-level zero-argument function named `main`, when it defines one.

Only a `main` declaration with exactly zero parameters is an entrypoint.
Defaulted and variadic parameters are not entrypoints, even when a call could
omit them. Imported functions named `main` are not entrypoints. A module without
a local zero-argument `main` finishes after top-level evaluation.

The selected function is called with no arguments. Evaluation completes with
either a Slug value or a language runtime error, and a top-level failure MUST
prevent entrypoint invocation.

Both whole-program evaluation and direct function evaluation establish an
implicit root task owner. The runtime must settle that owner before returning a
result to its caller:

- spawned descendants are joined before root settlement;
- an unawaited descendant failure is propagated from root settlement;
- an awaited task retains its completed result for repeated awaits;
- ordinary function calls do not themselves create or settle a nursery.

Closures retain their required lexical bindings. A runtime may optimize their
storage, but must preserve the sharing and capture rules in the task section.

## Evaluation and observable values

The runtime MUST preserve source evaluation order wherever it is observable.
In particular, map-literal entries evaluate in source order rather than host
map iteration order. Call arguments, list elements, map entries, struct fields,
pipeline operands, and match guards evaluate left to right before their
containing operation proceeds.

Each struct-schema evaluation creates a distinct schema identity. Field default
expressions evaluate once, in declaration order, during that schema evaluation.
Construction evaluates supplied fields in source order, then validates and
fills the schema-ordered value. A struct value's equality requires the same
schema identity and equal field values; schema equality itself is identity.

Only `false` and `nil` are falsey. Every other value is truthy. The runtime
MUST preserve the language-level distinctions
between initialization and reassignment so immutable bindings, unknown names,
and uninitialized values report the correct language error.

Runtime representations of functions and other program metadata must
be immutable after preparation when they can be shared by closures or tasks.
Applying tags or binding a closure must not mutate shared constants or global
singleton values.

The implementation MUST preserve value identity where it is observable:

- repeated references to the same channel refer to one communication endpoint;
- a task handle identifies one completion and retains its result;
- a closure observes its captured binding cells according to the capture rules;
- imported live bindings resolve against their defining module.

The Language Specification and `slug.test.assertEqual` define language-level
equality used by source fixtures. An implementation MUST NOT replace this with
host-language pointer or reference equality for lists or maps. Struct
implementations may use host identity to represent schema identity, but must
also compare the instance field values required by the language rule.

## Language errors and diagnostic context

An explicit `throw` and a VM-generated language fault use one catchable error
completion path. A language runtime error must retain:

- the thrown payload or fault payload;
- a source position and source path when available;
- an immutable snapshot of Slug call frames.

The following are language faults, rather than host panics: undefined names,
invalid calls, invalid assignments, missing required arguments, invalid
collection access, and supported-operation type errors.

Internal verifier failures and impossible VM states are implementation faults,
not catchable language errors. A conforming VM must report them predictably and
must not expose a host panic as ordinary Slug program behavior. Deferred helper
frames must not appear in a user-facing Slug stack trace.

## Scope cleanup and errors

Each lexical scope owns its deferred actions. Normal scope exit runs applicable
actions in last-in, first-out order:

- `defer` runs for either completion outcome;
- `defer onsuccess` runs only after successful completion;
- `defer onerror(name)` runs only while an error is unwinding and receives that
  error binding.

An `onerror` deferred action may recover by returning normally. If a deferred
action throws while another error is active, the new error replaces the active
one and records the prior error as its cause. An error produced by `await`
follows ordinary error unwinding and cannot bypass cleanup.

Recovery completes the handler's enclosing function with the handler's result;
its caller then continues normally. Deferred actions still pending in that
function run as successful cleanup, while the caller's scopes remain active.

## `recur`

`recur` is Slug's only stack-safe looping mechanism. The language has no
`while`, `for`, or `loop` construct, and no `break` or `continue` statement;
programs express repetition through recursive functions. A `recur(...)` in a
valid tail position restarts the current function rather than making a new
recursive call.

`recur` reuses the current function execution rather than growing either the
VM call stack or host call stack. It evaluates all next arguments before
rebinding function parameters.

The function-root scope persists between `recur` iterations. Deferred actions
registered in that scope persist and run exactly once, in LIFO order, on the
function's final return or error. A nested lexical scope abandoned by `recur`
exits normally, including its deferred actions, before the next iteration
begins. This distinction is defined by [ADR-040](/adr/adr-040).

Function-root nursery state and any associated limiter identity also persist
across a `recur` iteration.

## Tasks and nurseries

### Ownership and lifetime

Every evaluation begins in an implicit root nursery. `spawn` creates a task and
registers it with the current dynamic nursery. A nursery is an ownership and
lifetime boundary, not a lexical binding environment and not an ordinary
function-call boundary.

Exiting a nursery waits for its remaining children. Awaiting a task consumes it
from nursery failure propagation, while preserving its cached result for later
awaits. A child failure that remains unobserved is propagated when its owning
nursery settles.

An explicit nursery uses logical fail-fast behavior. Its first observed
unhandled child failure is retained, sibling tasks are cancelled with the
reason `sibling cancelled due to fail-fast`, and the nursery then settles. Root
settlement is intentionally not fail-fast: it joins descendants and propagates
the first temporal unobserved failure without physically interrupting host
execution.

Cancellation is logical. The runtime reports cancellation through task
settlement, but is not required to forcibly terminate a host goroutine or
thread.

### Limits

`nursery limit N` limits tasks spawned directly by the task that opened that
nursery. Descendant tasks remain owned by the nursery for lifetime and failure
propagation but do not consume the opener's limiter. A permit is released only
after the admitted task terminates.

The limit bounds admitted child tasks, not host threads or scheduler workers.
An admitted task remains admitted while it runs Slug code, waits to be scheduled,
or blocks inside a synchronous native call. Host worker availability determines
physical progress but does not change nursery ownership, admission, or permit
release. Implementations must state their worker and progress policy separately
instead of interpreting `limit N` as a promise of `N` operating-system threads.

### Spawn capture

When `spawn` executes, it snapshots only the immediate lexical binding cells:

- a parent reassignment after `spawn` is not visible through the captured local
  binding;
- captured values are shallow-shared, so channels and mutable objects retain
  their identity;
- outer lexical bindings remain live;
- root and loaded-module globals remain live and shared;
- ordinary, non-spawn closures continue to share captured mutable cells.

`../../tests/vm-conformance/supported/spawn-capture.slug` is the primary acceptance
fixture for this contract.

## `await` and `select`

`await` waits for task settlement and returns its cached result or propagates
its error through normal cleanup. Awaiting a task marks its failure as observed
by its owning nursery.

`select` evaluates a set of receive, send, timer, task-await, and default
cases. A selected task-await failure follows the same ordinary error path as a
standalone await. The exact fairness and tie-breaking policy for multiple ready
cases is intentionally not specified yet. Implementations must preserve the
observable behavior covered by the `select` VM conformance fixtures and must
not leak host-level select panics into Slug programs.

Immediate readiness inspection MUST NOT wait for an unsettled task or otherwise
allow an initially unready case to become ready before the remaining cases are
inspected. Registering a task-await case does not observe its failure; only
selecting that case does.

## Channels

`channel(capacity)` creates a channel with a non-negative integer capacity.
Channels preserve message and waiter order with FIFO queues. A send transfers
directly to the oldest waiting receiver when one exists; otherwise it buffers
until capacity is exhausted, then parks its task. A receive takes the oldest
buffered message, pairs with the oldest waiting sender, or parks its task.
Receiving from a closed, drained channel returns `nil`. Closing is idempotent,
wakes parked receivers with `nil`, and resumes parked senders with the ordinary
closed-send runtime error.

Parking a task MUST retain its execution state, including frames, operand
stack, lexical bindings, and pending deferred actions. Resumption MUST deliver
the pending call result or error through that retained state; it MUST NOT rerun
the task from its entrypoint or bypass cleanup. Root evaluations participate in
the same scheduler and may park until an owned task wakes them. If an owner
cannot settle a parked task because no runnable task or timer can make
progress, it reports a checked blocked-task runtime error. Explicit nursery
bodies participate in the same scheduler and may park. A live native producer
is a possible source of progress; its notification competes with ready tasks
and the nearest timer deadline rather than an implementation polling timeout.

Cancelling a parked task MUST remove its channel-send, channel-receive,
task-await, and timer registrations before it settles. A later operation MUST
NOT observe or communicate with a cancelled waiter.

## Required implementation isolation

The VM may use threads, goroutines, frames, environments, stacks, or slots
internally. Those choices are conforming only when they preserve the contracts
above. The VM must not import a concrete host runtime merely to acquire runtime
services. Keep the dependency direction from runtime orchestration into VM
execution, with host services injected at the boundary.

## Topics pending requirements

Future editions will define:

- a complete module initialization and cyclic-import contract;
- foreign-function calling, value adaptation, and resource ownership;
- scheduler fairness and precise `select` tie-breaking;
- VM validation requirements for malformed internal bytecode;
- memory management, resource limits, and host cancellation policy.
