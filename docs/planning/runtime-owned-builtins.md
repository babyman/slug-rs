# Runtime-Owned Builtins Plan

Implements [builtins belong to the runtime](../decisions/2026-11-15-builtins-belong-to-the-runtime.md).

## Scope and invariants

- Every program that compiles and runs now, and does not import
  `slug.builtin`, does the same afterwards, from source and as an artifact,
  on every host.
- The VM stays `no_std + alloc`. Check the ESP32-S3 firmware with each
  descriptor and the typed native adapter before committing.
- Interactive sessions keep looking unknown names up when they run, as the
  [unknown names](../decisions/2026-10-18-unknown-names-are-compile-errors.md)
  decision allows.

## 1. `Error` in the VM

- [x] Define the `Error` struct schema in `slug-vm` and install it as a VM
  builtin beside `cfg`, `int`, `stacktrace`, and `describe`. A runtime
  schema is the same wherever it is made, so the `Error` each module's VM
  installs is one type: equal values, equal schemas, and type tests agree.
- [x] The compiler counts `Error` among the VM's names, so it is known with
  no contract.
- [x] Tests: with no library root, a program constructs, throws, and catches
  `Error` from source and as an artifact, with the contract's defaults; an
  `Error` from another module is equal. (A type pattern `x:Error` is an
  `unknown type` error with or without the library; that predates this
  plan.)

## 2. The contract from the host

- [ ] Give the compiler the `slug.builtin` contract from its host, not from
  the resolver: `compile_with_resolver` and the interactive compiler stop
  resolving `slug.builtin`.
- [ ] The CLI, its server, and the REPL use `lib/slug/builtin.slug` built
  into the binary. The embedded build uses its descriptor's contract.
- [ ] `import("slug.builtin")` is a compile error naming the builtins as
  always available, and a runtime error for a dynamic import.
- [ ] Remove the runtime's `slug.builtin` imports: the VM installs host
  builtins from registrations only, and the virtual module goes.
- [ ] Rewrite the tests that import `slug.builtin`.
- [ ] Tests: a `slug/builtin.slug` beside a program, and in a project's
  `src/`, changes neither `Error` nor the accepted names; the compiler reports
  unknown names with no library root; the import error and its hint.

## 3. Artifacts

- [ ] Stop bundling `slug.builtin` in `compile_artifact`, and drop the
  artifact host's lookup for it. This replaces the bundling `f16bd1a` added.
- [ ] Remove the report's filter for the carried module.
- [ ] The firmware blink graph test expects three modules again.
- [ ] Keep `an_artifact_carries_the_implicit_builtin_module`, renamed: a
  compiled program still throws and catches `Error`.

## 4. Parity tests

- [ ] The CLI host: each `foreign` declaration in the built-in contract has a
  registered function with a matching arity, and each registered builtin is
  declared. Decide here how the slim build, without `chan` and `close`,
  relates to a contract that declares them.
- [ ] Each embedded target, over its contract and statically linked providers.
  The embedded build already fails on an undeclared or unimplemented foreign
  function; add `Error`.
- [ ] The contract's `Error` fields and defaults equal the VM's schema.

## 5. Documentation

- [ ] Specification: the "Implicit builtin module" section and the note on
  unknown names.
- [ ] `language/runtime-requirements.md` and
  `language/embedded-host-modules.md`.
- [ ] The header comments of `lib/slug/builtin.slug` and the embedded
  contract: they declare, the runtime defines, nothing imports them.
- [ ] Mark the [implicit builtin module](../decisions/2026-08-26-implicit-builtin-module.md)
  and [embedded builtin contract](../decisions/2026-10-09-embedded-builtin-contract.md)
  decisions superseded in part, and add a changelog entry.

**Gate:** full workspace, clippy, `make docs-check`, the ESP32-S3 checks, and
Orble's `make test`, `make build`, and `make test-smoke`.
