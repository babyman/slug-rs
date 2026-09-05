# Require C to retain values across producer backpressure

## Context

The initial C producer example sent one integer but did not prove the behavior
when a bounded receiver cannot accept another message. A later non-scalar API
must not silently lose or duplicate a value on this path.

This narrows the deferred retry-ownership work in
[`2026-09-03-c-ffi-prototype-producer-capability.md`](2026-09-03-c-ffi-prototype-producer-capability.md)
for integer payloads.

## Decision

The prototype names producer outcomes as `sent`, `full`, and `closed`. A C
caller retains an integer after `full` and may retry it later. `closed` means
the producer cannot accept that value; the C caller ends its work and destroys
the capability. The fixture uses a capacity-one channel, proves the second
send becomes full, and retries it only after Slug drains the first integer.

## Consequences

- Backpressure has a concrete no-loss rule before richer C-owned values exist.
- C controls retry timing; the host offers no blocking send or readiness
  callback.
- Strings, bytes, and structured values still require an explicit owned-value
  transfer contract before they can use this producer path.

## Migration

The unstable prototype minor version is now 5. Existing fixtures remain
compatible when rebuilt against the header.
