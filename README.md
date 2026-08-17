# Slug VM in Rust

This repository is a clean-room Rust implementation of the Slug language.
It starts with the execution boundary recommended by the language package: a
small, checked VM with internal Slug-specific bytecode. The bytecode is not a
file format or compatibility commitment.

## Current milestone

- Dynamic Slug values: `nil`, booleans, numbers, strings, bytes, symbols,
  lists, maps, closures, and explicitly registered native functions.
- Chunks, constants, lexical captures, locals, globals, calls, branches, and
  arithmetic/comparison operations.
- Checked errors with Slug source spans and call frames instead of host panics.
- Source execution for an initial core subset: `val`/`var` bindings,
  assignment, literals, arithmetic, calls, comments, and `println`.
- The lexer, parser, compiler, module loader, standard library, pattern matcher,
  and structured concurrency remain progressive milestones beyond this subset.

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

## Development

```sh
cargo test
cargo run -- --help
```

The integration tests construct small programs directly, covering arithmetic,
branching, closures, globals, native calls, and source-located runtime errors.
