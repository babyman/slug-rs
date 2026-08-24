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

A paired channel accounts for both the mailbox and the VM-resident queue
against one shared capacity. A producer must report `full` rather than create
a second unbounded queue. Releasing the last receiver revokes the producer,
leaving it closed.

## Consequences

Foreign-thread publication is limited to copyable scalar, string, and byte
payloads. Channel delivery and waiter resumption remain cooperative runtime
operations. A parked runtime polls its registered native mailboxes while it has
no runnable Slug work; an event-driven host wake is deferred until a native
host loop exists to own it.

## Migration

None. The version 0 Rust facade is unstable and no producer API previously
existed.
