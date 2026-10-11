# Generated shims for typed native calls

## Context

Typed native calls must bind a small first slice of filesystem-shaped C
functions without exposing VM values or callbacks. A bounded Rust dispatcher
can call those functions without a C build step, but a dynamic loader cannot
verify that a manifest-selected function-pointer type matches the library's
actual declaration. That mismatch is undefined behavior.

The typed native-call dispatch spike compared that path with generated C
adapters that include the declaration-owning header. Both candidates passed the
same direct fixture and checked Slug boundary failures. In the local scalar
comparison, the generated adapter had no measurable cost disadvantage versus
the 0.13 callback baseline or the Rust dispatcher.

## Decision

Header-backed typed native bindings use generated C shims for the first
implementation. The generator emits one adapter translation unit per binding
set, and that unit includes the upstream or clutch-owned header declaring the
target symbols. It makes exactly one direct target call per adapter call.

The binding build compiles the generated translation unit together with a
dynamic clutch library or the native objects of a static host. The Rust loader
continues to own dynamic loading and all unsafe calls; the VM and frontend do
not gain unsafe code or a general C-call facility.

The bounded Rust dispatcher from the spike is not a production fallback. It
may remain only while the spike artifacts exist, as evidence for the finite
shape set and for a future separately reviewed use case where no declaration
header is available.

## Consequences

Generated adapters turn wrong parameter or result declarations into C compiler
diagnostics when the included header is authoritative. Clutch authors need a C
toolchain during binding builds and must provide the header or an installed
library's equivalent development header.

This decision does not define manifest syntax, native error-code mapping,
resource-destruction metadata, text-result ownership, structs by value,
variadics, callbacks, or `libffi` use. Those remain separate decisions.

The prototype 0.13 ABI remains frozen until its planned replacement is ready.
This record narrows the implementation strategy anticipated by
[Typed native calls](2026-10-10-typed-native-calls.md); it does not supersede
that record's source-level or lifecycle decisions.

## Migration

None. The spike fixture is private, and existing Slug programs and the 0.13
prototype ABI remain unchanged.
