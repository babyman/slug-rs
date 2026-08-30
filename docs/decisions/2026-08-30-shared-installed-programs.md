# Share installed programs across VM executions

## Context

The VM previously cloned an entire private `Program` whenever it created a
task or explicit nursery body. The bytecode is immutable after construction,
so those copies added task-count-proportional storage and work without
isolating any mutable execution state.

## Decision

A public VM invocation installs one immutable `Rc<Program>`. Root closures,
spawned tasks, explicit nursery bodies, and module-relative closures clone
that owner rather than cloning its program data. The existing `&Program`
entry points remain as checked compatibility wrappers and make one copy to
establish the owner. Callers that already retain an `Rc<Program>` use the
installed entry points to avoid that copy as well.

## Consequences

Program storage is constant as tasks and nested nurseries are created; only
task-specific frames, stacks, captures, and suspended state grow. Opt-in VM
metrics report whole-program clone count and estimated copied inline
instruction bytes. The direct-bytecode API remains private to this Rust crate
and no executable representation becomes a compatibility contract.

## Migration

None for Slug source programs or existing direct-bytecode callers. Hosts that
reuse one program across invocations may retain it in `Rc<Program>` and call
the installed execution methods to remove the root installation copy.
