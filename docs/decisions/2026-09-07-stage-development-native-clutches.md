---
title: ADR-000 Stage native clutches in development repositories
date: 2026-09-07
---

## Status

Accepted for the local version-0 experiment.

## Context

The repository index names `slug.io.fs`, `slug.sqlite`, and `slug.math`, but
only the filesystem clutch had a committed platform binary. SQLite and math
were proven by temporary integration layouts, so ordinary local imports could
discover their manifests but not load a selected library.

## Decision

Treat this checkout's `clutch/` directory as a development repository. Add
`make stage-native-clutches`, which compiles each current-platform native
source adapter into the exact library path selected by its clutch manifest.
The staged libraries are ignored local outputs. A developer stages them before
running a program that imports a native clutch.

This clarifies the local-native installation decision in
[`2026-09-07-local-native-filesystem-clutch.md`](2026-09-07-local-native-filesystem-clutch.md).

## Consequences

### Positive

Every indexed local provider can be built and exercised through ordinary CLI
resolution on a supported development host. The repository no longer
advertises source-only adapters as if they were already packaged binaries.

### Negative

Development now requires a C compiler, and the SQLite adapter also requires a
local SQLite development library. Staging supports only the host platforms
implemented by the script.

### Neutral

This is not package installation, cross-compilation, signing, a lockfile, or
an ABI-v1 compatibility commitment.

## Migration

Before importing a native clutch from this checkout, run
`make stage-native-clutches`.
