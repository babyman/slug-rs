# Share installed programs across VM executions

## Context

The VM previously cloned an entire private `Program` whenever it created a
task or explicit nursery body. The bytecode is immutable after construction,
so those copies added task-count-proportional storage and work without
isolating any mutable execution state.

## Decision

A VM owns creation of an immutable `InstalledProgram` by taking ownership of a
mutable `Program` and validating one root entry. Root closures, spawned tasks,
explicit nursery bodies, and module-relative closures clone the installed
program's private `Rc<Program>` owner rather than cloning its data. The
existing `&Program` entry points remain checked compatibility wrappers and
make one copy before installation.

`InstalledProgram` is opaque: callers may inspect its immutable bytecode but
cannot mutate or construct its owner directly. `Vm::run_installed` and related
methods accept this object rather than `Rc<Program>`, so execution cannot
bypass installation validation. It is strictly an in-process runtime object;
source loading, byte loading, and the portable `.cslug` format remain separate
future work.

## Consequences

Program storage is constant as tasks and nested nurseries are created; only
task-specific frames, stacks, captures, and suspended state grow. Prepared
programs can be reused across VM instances without cloning or revalidating
their bytecode. Opt-in VM metrics report whole-program clone count, estimated
copied inline instruction bytes, and installation validation count. The
direct-bytecode API remains private to this Rust crate and no executable
representation becomes a compatibility contract.

## Migration

None for Slug source programs. Direct-bytecode hosts that reuse a program now
call `Vm::install` or `Vm::install_named` once and retain the resulting
`InstalledProgram` for execution.
