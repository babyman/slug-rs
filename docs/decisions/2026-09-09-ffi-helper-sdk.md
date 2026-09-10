# Add a private FFI helper SDK

## Context

The ABI 0.13 primitive producer boundary is intentionally explicit, but a
durable native event source otherwise repeats the same mutex, receiver-handle,
worker, bounded-backpressure, and teardown code. The stdin adapter exposed
that repetition before additional native event sources multiplied it.

## Decision

Provide `include/slug_ffi_helpers.h` as a header-only convenience layer over
the private prototype ABI. Its initial durable async-stream helper creates one
channel on first callback use, returns cloned receivers for later calls, starts
a native worker, closes its producer when that worker returns, and coordinates
teardown without exposing callback-context access to the worker.

The primitive ABI remains the authority for ownership and thread-safety. The
helper is not a released ABI and does not change ABI version 0.13.

## Consequences

Native Clutch authors can express ordinary stream workers in terms of event
acquisition and helper sends. Sources with distinct backpressure, lifecycle,
or event-loop requirements may continue to use primitive producer operations.
The helper must retain its synchronization guard through receiver-result
cloning and must keep code loaded until a detached worker returns.

## Migration

`slug.io.stdin` uses the helper. Existing imports and the primitive ABI remain
unchanged. Future native Clutches may adopt it incrementally.
