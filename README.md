# Slug VM in Rust

This repository is a clean-room Rust implementation of the Slug language.
It starts with the execution boundary recommended by the language package: a
small, checked VM with internal Slug-specific bytecode. Private VM bytecode is
not a file format or compatibility commitment; the planned, portable
compiled-module contract is documented separately as `.cslug`.

## Current milestone

- Dynamic Slug values: `nil`, booleans, numbers, strings, bytes,
  lists, maps, closures, and explicitly registered native functions.
- Chunks, constants, lexical captures, locals, globals, calls, branches, and
  arithmetic/comparison operations.
- Checked errors with Slug source spans and call frames instead of host panics.
- Source execution for a core subset: lexical `val`/`var` bindings, assignment,
  literals, arithmetic/comparisons/logic, functions and captures, blocks, `if`,
  lists/maps/indexing, calls, comments, and `println`.
- The module loader, standard library, pattern matcher, type annotations,
  structured concurrency, and the remaining language forms are progressive
  milestones beyond this subset.
- Portable `.cslug` compiled modules are an adopted compatibility target; no
  encoder or loader is implemented yet.

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
