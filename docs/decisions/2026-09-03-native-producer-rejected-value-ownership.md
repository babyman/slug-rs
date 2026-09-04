# Retain rejected native producer values

## Context

The native extension contract says that `try_send` transfers an owned send
value only when a bounded native mailbox accepts it. The version 0 Rust facade
accepted the value by ownership but returned a status without returning it on
`full` or `closed`. A producer that wanted to retry a string or bytes event had
to clone it before every attempt, and the facade disagreed with the planned
binary ABI.

## Decision

`NativeChannelProducer::try_send` returns the rejected `NativeSendValue` in
its `Full` and `Closed` statuses. `Sent` alone transfers ownership to the
runtime. Native producers may retry the returned value, coalesce it, or drop
it according to their own documented backpressure policy.

## Consequences

- The static Rust facade now enforces the ownership rule intended for version
  1 C modules.
- Retrying a rejected string or bytes event does not require a defensive clone.
- Native producer callers must handle the returned value for non-sent statuses.
- Stress coverage verifies that concurrent senders receive a value back when
  revocation closes a producer.

## Migration

The version 0 Rust facade is unstable. Callers matching `Full` or `Closed`
must bind or discard the returned value.
