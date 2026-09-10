---
title: ADR-001 Include math in the core Clutch
date: 2026-09-10
---

## Status

Accepted

## Context

`slug.math` is a bundled, stateless standard module, but it was installed as
its own native Clutch. That added a manifest entry, staged library, and dynamic
load without providing an isolation boundary distinct from the bundled core
modules.

## Decision

`slug.core.clutch` owns `slug.std`, `slug.io.stdin`, and `slug.math`. Its one
native library exports descriptors for all three module identities. Programs
continue to import math with `import("slug.math")`.

This supersedes the two-module package scope in
[`2026-09-09-core-clutch-stdin.md`](2026-09-09-core-clutch-stdin.md).

## Consequences

### Positive

The bundled standard modules stage and load through one native library, while
the native loader continues to prove a library may serve multiple modules.

### Negative

Building the core library now requires the platform math library.

### Neutral

The common package does not make module state or APIs shared. Filesystem and
SQLite remain separate Clutches because they represent distinct capabilities.

## Migration

Existing programs continue to use `import("slug.math")`. Development checkouts
continue to use `make stage-native-clutches`; it now builds math into
`libslug_core`.
