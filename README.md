# Slug VM in Rust

This repository is a clean-room Rust implementation of the Slug language.
It starts with the execution boundary recommended by the language package: a
small, checked VM with internal Slug-specific bytecode. Private VM bytecode is
not a file format or compatibility commitment; the planned, portable
compiled-module contract is documented separately as `.cslug`.

## Current milestone

- Dynamic Slug values: `nil`, booleans, numbers, strings, bytes,
  lists, maps, struct schemas and values, closures, and explicitly registered
  native functions.
- Chunks, constants, lexical captures, locals, globals, calls, branches, and
  arithmetic/comparison operations.
- Checked errors with Slug source spans and call frames instead of host panics.
- Source execution for a core subset: lexical `val`/`var` bindings, including
  list and map destructuring, assignment, integer, floating-point, hexadecimal,
  byte, boolean, nil, string, list, and map literals, arithmetic/comparisons/logic, functions and captures, blocks, `if`,
  literal/list/map and type-constrained `match` with computed map keys and non-binding case alternatives,
  pinned `^name` comparisons, `name @ pattern` bindings, and named or
  anonymous final rests, struct schemas with optional field annotations, construction and field access,
  function match bodies, `return`,
  `throw`, `defer` including `onsuccess` and `onerror` recovery, tail-position `recur(...)`,
  lists/maps/indexing, list slices, directional list append/prepend, pipeline calls and matches, positional/named/defaulted/variadic calls, positional call and list-literal spreads (except static overload-set calls),
  declaration and parameter tags with evaluated arguments, declaration-attached
  documentation blocks with retained module metadata (but without metadata
  introspection), comments, automatic invocation of a local zero-argument
  `main`, cooperative `spawn` task handles with `slug.channel.await`, explicit
  nurseries, bounded FIFO channels through the `slug.channel` library,
  `select` receive/send/timer/task-await/default cases, and implicitly
  imported `slug.builtin.println`.
- Source-level `import(name, ...)` with checked string module names,
  importer-relative and project-root resolution, `$SLUG_HOME/lib` library
  fallback, cached isolated module initialization, and string-keyed exported-value maps.
- Canonical source-annotation resolution in every compiler mode, including
  checked built-in type names and constructor arity, including distinct
  `schema` values and nominal `struct<S>` construction. Optional `-type-check`
  validation distinguishes non-nil `any` from universal `any|nil`, normalizes
  unions, compares structured annotations reflexively, checks statically known
  operator and collection-access operands, narrows direct nil checks in
  control-flow paths, diagnoses closed typed-match coverage, preserves precise
  collection results, checks fields through known schemas, and retains inferred
  function-value input and result types for higher-order positional calls.
- Lexically scoped semantic bindings retain ordered callable signatures through
  local declarations, aliases, parameters, and nested blocks. Calls to locally
  known callables undergo mandatory shape, generic, and parameter-type
  resolution in every compiler mode. Loader-backed compilation caches exported
  callable snapshots and preserves them through module members, explicit map
  destructuring, and `{*}` selection. Statically selected overloads lower their
  canonical input identity into private call bytecode; the VM invokes that
  exact member of the current live binding without runtime type validation.
- Immutable configuration collection from library and project TOML, `SLUG__`
  environment variables, and program options; source access through `cfg` is a
  subsequent milestone.
- The standard library, full type inference, and the remaining language forms are progressive
  milestones beyond this subset.
- Portable `.cslug` compiled modules are an adopted compatibility target; no
  encoder or loader is implemented yet.
- Statically registered native functions use an opaque, call-scoped version 0
  Rust facade with structured errors and typed resources. It remains unstable;
  no public C ABI or dynamic native loader exists yet.
- Top-level `foreign` declarations resolve through a module-qualified,
  host-registered native-function registry. The bundled `slug.channel` module
  uses this boundary for channel creation and closing.
- A metadata-backed, syntax-focused conformance suite derived from the legacy
  Slug corpus runs with the integration tests.

## Bytecode design

`Program` owns indexed `Chunk`s. A `Chunk` owns its `Constant` pool and a list
of `Instruction`s. Each instruction uses a typed `Op` enum, not numeric opcode
bytes. This makes compiler/VM validation explicit while the instruction set is
still changing. A compiler can attach a `SourceSpan` to any instruction, and
the VM keeps the active call frames on failures.

The VM uses an operand stack. Function calls use separate frame-local slots,
with closures copying only the declared captured slots. The current model
intentionally favors clear semantics and diagnostics over compact bytecode or
performance.

## Portable compiled modules

`.cslug` will be a versioned, portable compiled-module format. It will remain
separate from `Program`, `Chunk`, and `Op`, which are private Rust structures
and may change freely. See [compiled artifacts](docs/compiled-artifacts.md)
for the adopted contract and the requirements before version 1 is implemented.

## Development

```sh
make check
cargo run -- --help
```

`make check` runs formatting validation, Clippy with warnings denied, and all
unit and integration tests. Use `make test-vm` or `make test-cli` for the
focused test loop. Agent-specific development rules and language-change
workflow guidance are in [AGENTS.md](AGENTS.md).

The integration tests construct small programs directly, covering arithmetic,
branching, closures, globals, native calls, and source-located runtime errors.

## Documentation

The [documentation index](docs/README.md) defines the authority of language
specifications, architecture notes, development process, and compatibility
promises. The [language support matrix](docs/generated/language-support.md)
separates the target language specification from the currently implemented
Rust subset.
