# FFI channel producer prototype ABI 0.13 implementation plan

## Purpose

Move `slug.io.stdin` out of the core executable only after a native Clutch can
create a channel during a foreign call and safely keep the paired producer on
its own worker thread. This work is the next continuation of the experimental
native FFI: it targets `SLUG_FFI_PROTOTYPE_ABI_MINOR 13`, not a public version
1 ABI. It does not make the general VM, call context, or arbitrary Slug values
thread-safe.

The target consumer is a `slug.io.stdin` Clutch. Sockets, timers, filesystem
watchers, and device event sources are deliberately validation consumers of the
same capability, not reasons to widen the first ABI.

## Review of the proposed requirements

The proposal's central direction is accepted:

- callback contexts and values borrowed from them are valid only until the
  foreign callback returns;
- a channel producer is an explicit owned capability, not a generic retained
  Slug value;
- foreign threads may construct only owned scalar, text, and byte payloads and
  may only publish them through that capability; and
- shutdown must revoke producers without leaving a path to freed VM state.

The current implementation already establishes much of this behavior, but only
behind unstable boundaries:

| Requirement                                            | Current evidence                                                                        | Remaining ABI 0.13 work                                                             |
|--------------------------------------------------------|-----------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------|
| Rust producer survives its callback and is thread-safe | `NativeCall::channel`, `NativeChannelProducer`, and VM concurrency tests                | Specify and freeze equivalent C ownership rules.                                    |
| Bounded non-blocking delivery and retry ownership      | Rust returns rejected `NativeSendValue`; C fixtures cover integer and text backpressure | Cover every ABI 0.13 payload and make the C result/ownership contract explicit.     |
| Receiver loss and runtime teardown are safe            | paired receiver drop revokes its producer; native ABI contract specifies tombstones     | Prove C-level shutdown/revocation and define the ABI lifecycle entry points.        |
| C worker can create, retain, and destroy a producer    | prototype `channel_create`, integer send, text send, and `producer_destroy`             | Add close, the remaining scalar/bytes sends, and a stable header/conformance suite. |

Two adjustments keep the boundary narrow:

1. **ABI 0.13 exposes `sent`, `full`, and `closed`, not a separate
   `no_receivers` status.** Receiver disappearance, explicit channel close,
   and runtime revocation all mean that the producer must stop. Distinguishing
   them leaks a receiver-lifetime detail without changing the correct native
   action. `closed` is therefore the public cancellation signal.
2. **ABI 0.13 exposes only `try_send`.** A blocking producer send would allow a
   foreign event-loop or worker thread to become part of VM backpressure
   scheduling. Producers retain a rejected payload on `full` and choose their
   own retry, coalescing, or discard policy. A blocking `send` can be proposed
   later only with a concrete cancellation and shutdown design.

The current reference contract mentions producer cloning. That remains an
internal Rust facility, but ABI 0.13 deliberately omits a C clone or retain
operation: one producer returned from channel creation is sufficient for the
stdin migration. A later consumer must justify independently owned producers
before that API is frozen.

## Proposed ABI 0.13 prototype additions

The current prototype header owns the names. ABI 0.13 appends these operations
to its host table, preserving the existing `slug_ffi_*` naming and table-size
negotiation:

```c
typedef struct slug_ffi_channel slug_ffi_channel;
typedef struct slug_ffi_producer slug_ffi_producer;

slug_ffi_channel *channel_create(
    slug_ffi_call *call, uint64_t capacity, slug_ffi_producer **out_producer);
bool set_channel(slug_ffi_call *call, slug_ffi_channel *channel);
void channel_destroy(slug_ffi_channel *channel);

slug_ffi_producer_status producer_send_nil(slug_ffi_producer *producer);
slug_ffi_producer_status producer_send_bool(slug_ffi_producer *producer, bool value);
slug_ffi_producer_status producer_send_i64(slug_ffi_producer *producer, int64_t value);
slug_ffi_producer_status producer_send_f64(slug_ffi_producer *producer, double value);
slug_ffi_producer_status producer_send_text(slug_ffi_producer *producer, slug_ffi_text value);
slug_ffi_producer_status producer_send_bytes(slug_ffi_producer *producer, slug_ffi_text value);
void producer_close(slug_ffi_producer *producer);
void producer_destroy(slug_ffi_producer *producer);
```

`slug_channel_create` and `slug_call_set_channel` are runtime-call-thread-only.
All producer operations are thread-safe. The producer operation result is one
of `sent`, `full`, `closed`, or an argument/handle-contract failure defined by
the header. A released producer is invalid and must not be used again;
explicit release is separate from close.

On `sent`, the runtime has copied the payload into its owned channel mailbox.
On `full`, `closed`, or invalid input, native code still owns its input. Text
and bytes are `(pointer, length)` inputs; the runtime validates and copies
them before reporting `sent`, so their buffer need only remain valid for the
call. The initial text encoding is UTF-8; bytes are unconstrained. No API
constructs, sends, or retains arbitrary `slug_value` values.

`close` is idempotent and prevents future sends while preserving already
accepted FIFO messages for the receiver. `release` relinquishes the sole
native ownership lease. A producer remains useful after a receiver is dropped
only long enough to report `closed`; it never retains the VM merely because a
native thread failed to release it.

## Execution plan

### 1. Lock the internal lifecycle invariant

- Trace producer ownership through `Channel::native`, receiver drop, VM
  shutdown, module/Clutch cleanup, and detached worker threads.
- Make an explicit test matrix for close-before-send, close-after-queued-send,
  full/retry, receiver drop, VM shutdown with an outstanding producer, and
  concurrent close/send races.
- Ensure every failure is a checked runtime or ABI status; no producer path
  may panic, enter the scheduler directly, or dereference runtime state after
  revocation.

**Exit:** Rust VM tests demonstrate a safe tombstone-like closed outcome for
every outstanding producer and preserve FIFO messages accepted before close.

### 2. Implement prototype ABI 0.13

- Bump `SLUG_FFI_PROTOTYPE_ABI_MINOR` from 12 to 13 and append the new host
  table entries; descriptors must select ABI 0.13 exactly.
- Add a distinct `producer_close` operation; `producer_destroy` remains the
  prototype spelling of release.
- Add non-blocking bool, float, nil, and byte producer sends alongside the
  existing integer and UTF-8 text operations.
- Apply the same ownership rule to each payload: only `sent` transfers input;
  `full`, `closed`, and validation failure leave it with the C caller.
- Audit `channel_create`/`set_channel` failure paths so a callback that cannot
  return its channel closes and releases the producer and destroys its channel
  handle exactly once.
- Keep ABI 0.13 private to manifest-selected Clutches; it is an experimental
  compatibility increment, not a public native-module ABI.

**Exit:** focused C fixtures compile on supported platforms and prove each
payload, close/release separation, backpressure retry, receiver revocation,
and callback-return-before-worker-send.

### 3. Extract `slug.io.stdin` into a native Clutch

- Move the `readLines` foreign implementation and its worker state from
  `src/main.rs` into a `slug.io.stdin` Clutch with a source wrapper that keeps
  the existing `slug.io.stdin` source API unchanged.
- Have its callback create one bounded channel, publish the receiver, and pass
  only the producer to the stdin worker. The worker retries a retained line on
  `full`, stops on `closed`, then calls close and release exactly once.
- Give the Clutch an explicit cancellation path compatible with the ABI 0.13
  producer lifecycle. Do not retain a `NativeOwnedValue`, call context, or VM
  reference in global stdin state.
- Remove the core registration only after the external Clutch passes the
  existing stdin behavior tests and new shutdown/revocation tests.

**Exit:** core VM and CLI contain no stdin-reader thread or retained Slug
channel value; `slug.io.stdin` remains source-compatible when its Clutch is
available and reports the ordinary unavailable-module diagnostic when it is
not.

### 4. Use the evidence to prepare a future public ABI

Do not publish version 1 as part of this work. If the prototype proves stable
across stdin and one additional event source, a later plan can define the
public C header, compatibility policy, and independent ABI conformance suite.
Until then, `docs/reference/native-abi.md` remains an architectural target,
not a declaration that ABI 0.13 has become public.

### 5. Follow-on consumers and extensions

Validate one additional event source (timer, socket, or watcher) before
considering compound payloads, producer cloning, blocking sends, readiness
callbacks, or a separate receiver-loss status. Each proposed expansion must
state ownership across backpressure, cancellation behavior, and whether it
would require broader VM thread safety.

## Verification matrix

| Boundary           | Required proof                                                                                                                                  |
|--------------------|-------------------------------------------------------------------------------------------------------------------------------------------------|
| Rust producer      | `make test-vm` covers queue capacity, FIFO delivery, cross-thread send, close races, receiver drop, and shutdown.                               |
| C ABI              | ABI conformance fixtures cover every send form, full retry, close then release, callback-return lifetime, and malformed handles/descriptors.    |
| Clutch loading     | Dynamic-loader tests reject mismatched versions/tables before registration and retain code safely through teardown.                             |
| Stdin migration    | CLI tests preserve normalization, EOF/read-error closure, shared-stream behavior, backpressure, and shutdown without a core-owned Slug channel. |
| Repository handoff | `make check`, `make docs-check`, and `git diff --check` pass.                                                                                   |

## Non-goals

- Generic retain/release or persistent roots for arbitrary Slug values.
- Cross-thread call-context or VM access, native-to-Slug callbacks, or
  scheduler wake APIs.
- A public producer clone/retain operation.
- Blocking foreign-thread sends, unbounded native queues, or automatic retry.
- Changes to normal source-level channel semantics.
