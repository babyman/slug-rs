# Typed native producers

## Status

Accepted

## Supersedes

- [Add a private FFI helper SDK](2026-09-09-ffi-helper-sdk-superseded.md)
- [FFI worker library leases](2026-09-15-ffi-worker-library-leases-superseded.md)

## Context

[Typed native calls](2026-10-10-typed-native-calls.md) keeps `chan` for
events: the Slug program creates the channel, and native code receives a
producer capability minted from it. It leaves open how the capability crosses
a typed binding, how native code reaches the host's send functions, and how a
Slug-created channel accepts native sends.
[Step 3 of the typed native-call plan](../planning/completed/typed-native-calls-step-3.md)
needs all three to move `slug.io.stdin`.

The VM supports native sends only into a channel that native code created:
`NativeCall::channel` makes the channel and its producer queue together, and
the VM drains the queue and registers the channel for progress wakeups only
when such a channel is a call's result. A channel made by `chan(n)` has no
queue. Native and Slug sends share one capacity: a Slug send reserves a slot
in the producer's occupancy count.

The producer records fix the remaining semantics:
[capability](2026-09-03-c-ffi-prototype-producer-capability.md),
[backpressure](2026-09-04-c-ffi-prototype-producer-backpressure.md),
[revocation and text ownership](2026-09-04-c-ffi-prototype-producer-revocation-and-text-ownership.md),
and [rejected-value ownership](2026-09-03-native-producer-rejected-value-ownership.md).

## Decision

**A `producer` argument.** A typed signature may take an argument of type
`producer`, as in `(producer lines) -> i32`. Its Slug annotation is `chan` or
`chan<T>`. The adapter mints a producer capability from the channel argument
and passes C an opaque `slug_producer *`. Each call mints a new capability,
which C owns until it calls release. A binding with a `producer` argument
needs `profile = "events"`, so a target that offers only `core` rejects it at
build time. `producer` is an argument type only, and its canonical spelling is
`producer`. There is no Slug-visible minting builtin.

Minting from an unbuffered channel, capacity 0, fails with `native.type`,
because no native send could ever succeed. Minting from a closed channel
succeeds, and every send returns closed.

**Host functions through the capability.** `slug_producer` begins with a
pointer to a host function table:

```c
typedef struct slug_producer slug_producer;
typedef struct slug_producer_api {
  uint32_t version; /* 1 */
  int32_t (*send_nil)(slug_producer *producer);
  int32_t (*send_bool)(slug_producer *producer, bool value);
  int32_t (*send_i64)(slug_producer *producer, int64_t value);
  int32_t (*send_f64)(slug_producer *producer, double value);
  int32_t (*send_text)(slug_producer *producer, const char *text, size_t length);
  int32_t (*send_bytes)(slug_producer *producer, const uint8_t *data, size_t length);
  bool (*is_closed)(slug_producer *producer);
  void (*close)(slug_producer *producer);
  void (*release)(slug_producer *producer);
} slug_producer_api;
struct slug_producer { const slug_producer_api *api; };
```

A header, `slug_producer.h`, declares these types, the statuses
`SLUG_PRODUCER_SENT` (0), `SLUG_PRODUCER_FULL` (1), `SLUG_PRODUCER_CLOSED` (2),
and `SLUG_PRODUCER_INVALID` (3), and static inline wrappers such as
`slug_producer_send_text(producer, text, length)`. Library code has no global
host state, and no symbol can clash between two providers linked into one
image. This refines the typed-calls record's "host function set at
initialization": the set is the same, but the capability carries it.

Every function may be called from any thread, and none blocks. Sends are
non-blocking: `full` asks C to retry later, and `closed` means the receiver or
runtime has gone away, so C should stop sending and release. After release, C
must not use the pointer again.

**Values are copied.** Text and bytes are borrowed for the duration of the
send: the host copies them and C keeps its buffer, whatever the status. Text
must be valid UTF-8, or the send returns `invalid`. This keeps the
rejected-value rule, that C owns what was not sent, without the prototype's
destructor transfer on `sent`.

**Producers on Slug channels.** A channel gains its producer queue the first
time a producer is minted from it. The queue's occupancy starts at the number
of messages already buffered, so native and Slug sends keep sharing the
channel's capacity. Later mints share the same queue, with counted leases.
The VM drains the queue and registers the channel for progress wakeups as it
does for a native-created channel, including when the channel was a call's
argument. Close, revocation when the receiver or VM goes away, and lease
accounting behave as the producer records specify.

**Library lifetime.** A dynamic library that has received a producer stays
loaded for the process lifetime, as the prototype already does, because a C
worker may still be returning through library code after its last send.

**The header is generated.** The shim build writes `slug_producer.h` beside
the generated shim and adds that directory to the include path for the
clutch's sources, so a library includes it without a copy. The header carries
the same `slug_producer_api` version as the adapter.

## Consequences

Native code can stream values into any buffered channel a Slug program
creates. Slug code keeps ownership of the channel and its capacity, and the
producer interface needs no initialization hook and no Slug-visible builtin.
The adapter implements the host functions over the VM producer; its new
`unsafe` code is limited to the functions C calls. The VM's channel gains a
settable producer slot. Its scheduling and progress tracking are unchanged
for every existing channel.

Still unsupported: producers on embedded targets, which offer only the `core`
profile; sending structs, buffers, lists, or maps; and a blocking send.

## Migration

None. The prototype's native-created channels are unchanged until step 4
removes the prototype.
