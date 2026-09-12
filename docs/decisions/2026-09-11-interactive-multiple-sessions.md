# Keep interactive sessions isolated in one VM

## Context

The interactive server must support several active sessions without assigning a
VM to each one. Ordinary bindings must remain private, while host embeddings
need a deliberate way to expose resources such as shared channels.

## Decision

The server owns exactly one VM and creates one compiler snapshot plus one
runtime environment for every `session.open`. It activates only the selected
runtime environment for each submission. `session.close` removes only that
pair of session-owned states.

Host-provided sharing crosses this boundary only through explicitly registered
host-native bindings, using `Server::define_host_native`. A host-native may
return a shared Slug value, including a channel, to more than one session. The
sessions then use that value through ordinary Slug operations; no session gets
access to another session's global environment.

## Consequences

Same-name bindings and closures are scoped to their originating session, and
closing one session leaves other session environments and the shared VM alive.
The host is responsible for the lifecycle and intended sharing semantics of
values returned by host-native bindings. Stalled or task-backed submissions are
not part of this decision and remain a later milestone.

## Migration

None.
