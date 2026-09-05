# Retain C-owned text until producer delivery succeeds

## Context

Integer retries prove ordering but not ownership. A C integration needs to know
whether it frees a heap buffer after the host accepts it, retains it across
backpressure, or releases it after the receiver has gone away.

This extends the integer rule from
[`2026-09-04-c-ffi-prototype-producer-backpressure.md`](2026-09-04-c-ffi-prototype-producer-backpressure.md)
and exercises the previously deferred receiver-revocation path.

## Decision

The text producer operation accepts length-delimited text and a C destructor.
The host copies and destroys the buffer only on `sent`. `full`, `closed`, and
invalid-text results leave ownership with C. A C worker that sees `closed`
releases its pending buffer and destroys its producer capability. The fixture
also proves a delayed send receives `closed` after Slug drops the receiver.

## Consequences

- C-owned buffers have an explicit exactly-once transfer point.
- Backpressure and receiver revocation do not silently leak or duplicate text.
- The host currently copies text; zero-copy payloads, bytes, lists, and
  structured values remain outside the prototype.

## Migration

The unstable prototype minor version is now 6. Existing fixtures remain
compatible when rebuilt against the header.
