# Adopt an opaque native extension interface

## Context

Slug needs channels and structured concurrency, and those features will be the
first substantial users of host resources, cross-thread events, and retained
values. The current native function type exposes the Rust `Value` enum directly
and returns unstructured strings. Extending that type for channels would make
private VM representation, `Rc` lifetimes, and scheduler details part of a host
contract before they are ready.

External FFI loading is a separate problem. Combining loading, arbitrary C
signatures, asynchronous execution, and VM calls in one interface would make
the concurrency implementation responsible for an unnecessarily large unsafe
boundary.

## Decision

Slug will use one opaque native extension interface for statically registered
host functions and future Slug-aware native modules. The detailed contract is
defined in [`../reference/native-abi.md`](../reference/native-abi.md).

Native callbacks are synchronous and receive a call-scoped opaque context.
They return one value or one structured error and cannot suspend, re-enter Slug,
or access tasks, nurseries, or the scheduler. Function registration carries no
execution class or scheduling hint. A blocking callback occupies its calling
Slug task until it returns; callers use `spawn`, while event-driven native code
publishes through channels.

Borrowed Slug values remain call-scoped. Native resources use module- and
type-checked opaque handles, with host synchronization owned by the native
implementation rather than a Slug-visible thread-safety flag. Foreign threads
may publish only owned scalar, string, or byte values through a thread-safe
channel producer capability; they never receive general VM values or scheduler
wake operations. Version 1 defers persistent roots and compound transferable
values until concrete consumers justify their lifetime and construction APIs.

Runtime shutdown revokes producer capabilities before destroying VM state.
Outstanding producers become closed tombstones that cannot keep the runtime
alive or reach freed runtime state. Uncooperative native work never causes code
to be unloaded or state still in use to be freed.

The initial Rust facade is version 0 and is intentionally unstable, but it must
enforce the planned lifetime and threading restrictions. A C-compatible native
module ABI version 1 will be published only after channels and concurrency have
validated the contract. Dynamic module discovery and raw C signature bridging
remain later, separate layers.

## Consequences

- Channels can test value retention, native resources, cross-thread delivery,
  close races, and teardown before a public binary layout is frozen.
- VM values, private bytecode, reference counting, task representation, and
  scheduler policy remain replaceable implementation details.
- Native registration does not create a second worker queue or resource budget
  beside Slug's nursery and scheduler model.
- Blocking libraries use synchronous calls inside ordinary spawned tasks;
  event-driven integrations return channels and retain producer capabilities.
- Native authors must copy or build transferable event payloads rather than
  sending borrowed VM values across threads.
- Native resource implementations must synchronize shared host state or reject
  concurrent use with a checked error.
- Native event payloads initially require scalar, string, or byte encodings;
  convenient compound payload construction is deferred.
- Native-to-Slug callbacks, module unloading, and automatic raw C binding are
  deliberately unavailable in version 1.
- Publishing version 1 requires a C header, version negotiation, loader
  validation, ownership rules, and ABI conformance tests in the same change.

## Migration

The existing Rust `NativeFunction = fn(&[Value]) -> Result<Value, String>` API
is pre-release and will be replaced by the version 0 facade. Existing internal
native functions and VM tests must migrate before channels are implemented. No
external binary modules exist.
