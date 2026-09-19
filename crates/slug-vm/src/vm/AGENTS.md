# Virtual Machine

This directory owns execution. `mod.rs` remains the single host-driving owner
of installation, dispatch, execution orchestration, and polling. Supporting
modules own focused runtime responsibilities: call-frame and local storage,
operand-stack access, cleanup/unwinding, runtime errors, operations, scheduler
state, timers, and progress.

Keep bytecode as the `Program`/`Chunk`/`Instruction`/`Op` compiler-to-VM
boundary. Preserve checked `RuntimeError` failures with source spans and call
frames; never replace an invalid program or runtime fault with a host panic.
Do not split dispatch mechanically by opcode. Native producers may enqueue
restricted owned values and signal progress, but may not execute Slug or mutate
VM-owned execution state directly.

Add bytecode/runtime regressions under the stable `tests/vm.rs` facade, using
its behavior modules: `bytecode`, `calls_and_native`, `collections`,
`concurrency`, `lifecycle`, or `runtime`. Run `make test-vm`; use
`cargo test -p slug-vm --no-default-features --test vm` for a focused
slim-runtime check. Run `make bench-vm` before and after a hot-path relocation,
without treating timings as a threshold.
