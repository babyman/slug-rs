# Use direct locals and promote escaping captures

## Context

Every frame local previously used a shared binding cell even when no closure
could observe it. This added allocation and interior-mutability overhead to
ordinary local reads and writes. Eagerly capturing every visible outer binding
also made closures retain bindings their bodies never referenced.

## Decision

Frame locals begin as direct `Value` slots. Creating a closure promotes each
captured local slot to a shared binding cell, and local reads and writes handle
either storage form. Closure captures remain shared cells so mutable sibling
and nested closures retain identity.

The compiler records captures lazily. An inner function captures only an outer
binding it references; when that binding lives beyond the immediate parent,
the parent receives the required intermediate capture.

`recur` replaces the active frame's local slots without mutating cells already
held by closures from an earlier iteration.

## Consequences

Ordinary frames avoid per-local cell allocation. Escaping closures preserve
their existing mutable-binding and iteration semantics. Runtime metrics count
only cells created by promotion, so the benchmark distinguishes ordinary local
storage from closure allocation.

The compiler carries deferred capture requests while compiling nested
functions. This is private bytecode construction machinery and does not change
source syntax, diagnostics, or the `.cslug` contract.

## Migration

None for Slug programs, private bytecode construction, or the future compiled
artifact format.
