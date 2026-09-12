# Overlay shared host bindings in interactive sessions

## Context

Interactive sessions need isolated bindings and closures while continuing to
use one VM and its host facilities. Cloning host globals when a session opens
does not make host bindings registered later available to that session.

## Decision

Each interactive environment retains an empty session-local global environment,
a reference to the VM host global environment, and the names declared by
successful session submissions. Immediately before executing a submission, the
VM copies every host binding whose name is not session-local into that session
environment. A session-local declaration shadows a host binding of the same
name. The VM still swaps the session environment in only for the execution and
restores its host environment afterward.

`Server::define_host_native` is the explicit server-engine entry point for
adding shared host-native bindings. The default output natives are installed
through that same VM host-binding mechanism.

## Consequences

Host bindings added after a session opens become available on that session's
next submission without exposing its ordinary bindings to any other session.
Closures retain their session environment, so shadowing remains stable after
the submission that introduced it.

This is an addition-oriented overlay: host binding removal is not currently a
server operation, so removing host globals is intentionally out of scope. The
previous decision's cloned-host statement is superseded by this decision.

## Migration

None.
