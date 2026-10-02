# Slug VM crate boundaries

This crate owns the compiler-to-VM boundary and all embedded-host integration.
Keep the crate-level exports in `src/lib.rs` stable unless an explicit API
change is intended; bytecode remains an in-process, unstable Rust boundary.

Source resolution, compiler snapshots, module-instance caching, and Clutch
activation belong to `slug-frontend` and the loader crates. The VM accepts only
opaque module-identity text for host import callbacks; do not reintroduce
loader contracts into this crate. Prove import behavior with `make test-modules`.

`src/native.rs` owns native descriptors, value conversion, resources, and
thread-safe producer ingress. Producers may enqueue owned values and signal
progress only; the VM is the sole owner that executes Slug and mutates runtime
execution state. Prove native/VM interaction in `tests/vm/calls_and_native.rs`
or `tests/vm/concurrency.rs`, then run `make test-vm`.

Keep `value.rs` with channels and tasks, and keep `collections.rs` with their
persistent backing representations. Do not introduce a directory or a new
crate solely to mirror a conceptual layer when it would obscure these lifecycle
and ownership relationships.
