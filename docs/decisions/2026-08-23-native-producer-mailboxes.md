# Native producer mailboxes

## Context

Native producers may be called from arbitrary host threads, while Slug channel
state contains VM values and task waiters that are owned by the cooperative
runtime thread. Sharing that state directly would expose task scheduling and
`Rc`-based VM representation across the native boundary.

## Decision

A native producer owns only a mutex-protected mailbox of restricted owned send
values. It never receives a Slug `Value`, task waiter, VM reference, or
scheduler handle. The VM thread converts accepted mailbox values and applies
ordinary FIFO channel delivery.

The eventual paired-channel implementation will account for both the mailbox
and the VM-resident queue against one channel capacity. A producer must report
`full` rather than create a second unbounded queue. Runtime teardown revokes
the producer before releasing the receiver, leaving the producer closed.

## Consequences

Foreign-thread publication is limited to copyable scalar, string, and byte
payloads. Channel delivery and waiter resumption remain cooperative runtime
operations. The mailbox needs an explicit wake mechanism in the next slice so
a parked root evaluation can observe a newly accepted event without polling.

## Migration

None. The version 0 Rust facade is unstable and no producer API previously
existed.
