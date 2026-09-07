# Deterministic Native Clutch Shutdown

## Context

The version-0 native clutch loader retained every dynamic library for the
process lifetime. Closing a native resource left its payload for Rust `Drop`,
and module teardown likewise depended on eventual dropping. That made a safe
`dlclose`/`FreeLibrary` impossible and contradicted the clutch lifecycle goal
of deterministic VM shutdown.

## Decision

At VM shutdown, each loader deterministically finalizes native clutch owners in
this order: stop new VM work; close and destroy tracked native resource
payloads into tombstones; destroy native module state; remove foreign
registrations; run the plugin's one-shot cleanup; and release its final dynamic
library lease. Native functions and resource creation paths share an active
lifetime state and report `native.plugin_inactive` after finalization rather
than calling unloaded code.

Cleanup is an owned one-shot operation which may report an error. Shutdown
continues finalizing other plugins and exposes collected failures through the
module loader. Live or hot unloading remains out of scope.

This supersedes the process-resident-code conclusion of
[`2026-09-06-experimental-clutch-boundary.md`](2026-09-06-experimental-clutch-boundary.md)
for the version-0 clutch prototype.

## Consequences

The dynamic loader has no process-global library cache. Plugin-owned state
holds the only lease and clears it after native state has been destroyed.
Resources must be finalized before plugin cleanup, because their destructors
are native pointers. Retained Rust values are safe tombstones after shutdown,
but hosts still must quiesce VMs sharing a loader before shutdown.

The current one-native-module-per-clutch limit is an experimental descriptor
layout limitation, not a clutch invariant. Archive clutches, `.cslug`, package
resolution, capability frameworks, and concurrency redesign remain out of
scope.

## Migration

Existing source imports and manifests need no changes. Tests that assumed a
resident library now assert a fresh module state after reload, and native
callbacks retained after shutdown receive the new structured inactive-plugin
error.
