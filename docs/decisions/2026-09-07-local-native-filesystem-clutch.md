---
title: ADR-000 Commit the local native filesystem clutch
date: 2026-09-07
---

## Status

Accepted for the local version-0 experiment.

## Context

The native filesystem layout was proven only in a temporary test fixture while
the repository's `slug.io.fs` clutch continued to select a Rust fallback. That
left ordinary CLI execution on a different implementation from the intended
clutch boundary.

## Decision

Commit the locally built macOS ARM library at
`clutch/slug.io.fs.clutch/native/macos-aarch64/libslug_io_fs.dylib`. The
canonical `slug.io.fs` manifest enables its `[native]` implementation and maps
the `macos-aarch64` platform to that file. Its C source remains alongside it in
`native/source/fs.c`.

Remove the Rust `slug.io.fs` facade and its CLI registration. A declared native
module never falls back to a host plugin. On any platform without a declared
library, importing the module returns the existing checked clutch error.

This supersedes the filesystem-facade migration statement in
[`2026-09-07-exploded-clutch-native-library-layout.md`](2026-09-07-exploded-clutch-native-library-layout.md)
and the test-only loading decision in
[`2026-09-07-test-built-filesystem-clutch-ffi.md`](2026-09-07-test-built-filesystem-clutch-ffi.md).

## Consequences

### Positive

The normal local CLI, source module, native resource implementation, and
shutdown path now exercise one installed clutch package.

### Negative

This checkout's filesystem clutch is intentionally macOS ARM-specific. Other
platforms receive a checked unavailable-platform diagnostic until their own
local library is added.

### Neutral

The committed dylib remains an unstable local experiment, not a packaged
release artifact or ABI-v1 promise. Future platform libraries and distribution
format require separate decisions.

## Migration

Remove `src/filesystem.rs` and host registration. Existing Slug programs keep
the same `import("slug.io.fs")` API on macOS ARM; on other platforms they must
handle installation failure until a matching local package binary exists.
