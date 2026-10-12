# Typed native calls, step 3: producers and `slug.core`

## Goal

Implement step 3 of the replacement plan in
[Typed native calls](../decisions/2026-10-10-typed-native-calls.md): add the
interface through which native code sends values into a channel that Slug
created, and move `slug.io.stdin` to it.

`slug.io.stdin` shares the `slug.core` clutch with `slug.math` and `slug.std`,
and a clutch has one native form, so the whole clutch moves. `slug.math` needs
only step 1 types. `slug.std` has no native code since `keys` became a
builtin. After this step no first-party clutch uses the 0.13 prototype, which
leaves only step 4: deleting it.

Step 3 ends when `slug.io.stdin` and `slug.math` run on typed bindings, their
tests pass on macOS, Linux, and Windows, and native code can send every
producer value type through a Slug-created channel.

## Out of scope

- Removing the prototype loader, its headers, the helper SDK, and the
  prototype test copies (step 4).
- Producers on embedded targets. The firmware descriptors offer only the
  `core` profile, and the static build already rejects an `events` provider.
- Structs, buffers, lists, and maps sent through a producer.
- Performance work, which stays deferred
  ([VM optimization item 11](vm-optimization.md#11-append-to-uniquely-owned-collections-in-place-deferred)).

## What exists today

- **Native-created channels only.** A channel gets its native producer queue
  when native code creates it (`NativeCall::channel`). A channel made by
  Slug's `chan(n)` has none, and the VM cannot attach one later. The VM drains
  a channel's producer queue into the channel and registers it for progress
  wakeups only when the channel is created that way.
- **`slug.io.stdin`.** `readLines()` is a prototype foreign function. On its
  first call the library creates the channel, starts a worker thread through
  the private helper SDK (`include/slug_ffi_helpers.h`), and returns a clone of
  the same receiver to every later call. The worker sends each line as text,
  retries with a 1 ms sleep while the channel is full, closes the channel at
  end of input, and destroys its producer.
- **`slug.math`.** `add` reports `math.range` on integer overflow, and `sqrt`
  reports `math.domain` for a negative number, both as native errors.
- **Settled semantics.** The producer records fix backpressure (`sent`,
  `full`, `closed`, `invalid`), revocation when the receiver or VM goes away,
  and that a rejected value stays owned by C.

## Decisions

Decisions 1-8 below were accepted as recommended on 2026-10-11, including
the diagnostic change in decision 6.

1. **How Slug hands a producer to native code.** Recommendation: a manifest
   argument type `producer`, whose Slug annotation is `chan`. The adapter
   mints a producer capability from the channel argument and passes C an
   opaque `slug_producer *`, which C owns until it releases it. There is no
   Slug-visible minting builtin; minting happens at the binding boundary, one
   capability per call. A binding with a `producer` parameter needs
   `profile = "events"`.
2. **How C reaches the host functions.** The decision record says the library
   receives a host function set at initialization. Recommendation: the
   capability carries it instead. `slug_producer` points to a host function
   table, and a header supplies static inline wrappers such as
   `slug_producer_send_text(producer, text, length)`. There is then no global
   state and no symbol that could clash between two providers in one static
   image. The set is the one the record lists: non-blocking sends of nil, bool,
   integer, float, text, and bytes, a closed-state query, close, and release.
3. **Text and bytes ownership.** Recommendation: the host always copies, and C
   keeps its buffer in every case. The producer records hand an owned buffer
   to the host on `sent`, with a destructor; copying always is simpler and
   keeps the rejected-value rule (C owns what was not sent). Invalid UTF-8 in
   text returns `invalid`.
4. **Attaching a producer to a Slug channel (VM change).** Recommendation: a
   channel gains its producer queue the first time a producer is minted from
   it, and later mints share that queue with counted leases. The VM drains it
   and registers it for progress exactly as for a native-created channel.
   Backpressure, revocation, and close behave as the producer records specify.
   This is an additive VM change, with VM tests for each path.
5. **The new `slug.io.stdin`.** Recommendation: the module creates one shared
   channel of capacity 32, as today. The first `readLines()` starts the reader
   by passing that channel to a raw `startReader(producer)` binding, and every
   call returns the same channel, so lines still go to one receiver each. The
   worker is plain C with platform threads; the helper SDK is not used.
6. **Errors from `slug.math`.** Recommendation: `add` and `sqrt` become Slug
   wrappers over raw functions that report overflow and a negative input as
   ordinary results. The wrappers throw `Error` values typed `math.range` and
   `math.domain`. As with `slug.io.fs` and `slug.db.sqlite`, an uncaught
   failure then reports the thrown struct. This needs your acceptance.
7. **Library lifetime.** Recommendation: never unload a library that has
   received a producer, as the prototype already does: a C worker may still be
   returning through library code after its last send.
8. **Where the producer header comes from.** Recommendation: the shim build
   writes `slug_producer.h` beside the generated shim and adds that directory
   to the include path, so clutch sources include it without a copy. A clutch
   author can also copy it for editing.

## Phases

### 1. Decisions

- [x] Record decisions 1-4, 7, and 8 in a decision record that extends
  [Typed native manifest bindings](../decisions/2026-10-11-typed-native-manifest-bindings.md),
  and decisions 5 and 6 in this plan:
  [Typed native producers](../decisions/2026-10-11-typed-native-producers.md).
  The record adds one rule the plan did not state: minting a producer from an
  unbuffered channel fails with `native.type`, because no native send into it
  could ever succeed.

### 2. Producers on Slug channels in the VM

- [ ] Mint a producer from an existing channel, sharing one queue per channel.
- [ ] Drain it and register it for progress like a native-created channel.
- [ ] VM tests for sends of each value type, `full` and `closed`, revocation
  when the receiver or the VM goes away, and close.

### 3. The `producer` type

- [ ] Parse `producer` in the manifest model, require the `events` profile,
  and accept the `chan` annotation in the compatibility check.
- [ ] Generate the `slug_producer.h` header and the host function table type.
- [ ] Mint and pass the capability in the adapter, and implement the host
  functions over the VM producer.
- [ ] Tests: a C fixture whose worker thread sends every value type, handles
  `full` by retrying, observes `closed` after the receiver is dropped, closes,
  and releases.

### 4. Move `slug.core`

- [ ] Rewrite `core.c` as plain C: the stdin reader and the math functions,
  with a header and a typed manifest using the `events` profile.
- [ ] Rewrite `stdin.slug` and `math.slug` as raw declarations plus wrappers
  that keep every public name, signature, and documented behavior.
- [ ] Stage the clutch through `slug-native-shim stage`.
- [ ] Keep the existing stdin and math tests unchanged, keeping a prototype
  copy only where a prototype test needs one, and add tests for the thrown
  `math` error types.

### 5. Measure and document

- [ ] Measure standard-input throughput, typed versus prototype, and record
  it.
- [ ] Update `native-abi.md`, `experimental-clutches.md`, and the README for
  producers and the moved clutch.

## Exit gate

- `slug.io.stdin` and `slug.math` run on typed bindings, and their tests pass
  on macOS, Linux, and Windows.
- A C worker can send every producer value type through a Slug-created
  channel, with the existing backpressure, revocation, and ownership rules.
- No first-party clutch uses the 0.13 prototype.
