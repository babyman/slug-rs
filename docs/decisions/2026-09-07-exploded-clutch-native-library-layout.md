---
title: ADR-000 Define the exploded clutch native-library layout
date: 2026-09-07
---

## Status

Accepted for the version-0 clutch experiment.

## Context

The filesystem clutch has a C implementation that the test suite builds and
stages manually. Its `clutch.toml` still names a Rust host plugin, so an
installed clutch has no declared native-library location or loader selection
rule. Continuing to inject the dynamic module from tests would leave the
experiment unable to prove its installed path.

## Decision

An exploded clutch declares an optional native plugin in its own manifest.
The module's existing `plugin` value names that declaration:

```toml
[modules]
"slug.io.fs" = { source = "modules/fs.slug", plugin = "slug.io.fs.native" }

[plugins."slug.io.fs.native"]
abi = "slug-ffi-prototype/0.7"

[plugins."slug.io.fs.native".libraries]
"macos-aarch64" = "native/macos-aarch64/libslug_io_fs.dylib"
"linux-x86_64" = "native/linux-x86_64/libslug_io_fs.so"
"windows-x86_64" = "native/windows-x86_64/slug_io_fs.dll"
```

The platform key is the host operating-system and architecture pair. Version 0
recognizes only the keys listed above; an absent current-platform entry is a
checked unsupported-platform error. Every library path is relative to the
clutch root, must remain inside it, and must name a regular file. The loader
does not search system library paths, infer a filename, or use a library from
another clutch.

`slug-ffi-prototype/0.7` selects the existing unstable descriptor initializer
and requires the descriptor's ABI-major and ABI-table validation to succeed.
The manifest selector is exact; it does not turn the prototype into an ABI-v1
promise or permit arbitrary C libraries. The normal experimental CLI build
contains this one loader path rather than using a Cargo feature to change an
installed clutch's behavior.

Loading remains transactional: resolve the selected file, load and validate
the descriptor, stage it through the module-scoped registrar, then compile and
publish the source module. The dynamic library remains process-resident after
plugin cleanup, exactly as decided in
[`2026-09-06-experimental-clutch-boundary.md`](2026-09-06-experimental-clutch-boundary.md).

## Consequences

### Positive

An exploded clutch now has a concrete, inspectable installed layout. The
filesystem vertical slice can exercise the same repository manifest, clutch
manifest, library selection, descriptor validation, import, resource cleanup,
and shutdown path used by the CLI.

### Negative

Each supported target needs a separately built library, and an unsupported
target fails explicitly instead of falling back to the Rust test facade.

### Neutral

This narrows the test-built dynamic-module decision in
[`2026-09-07-test-built-filesystem-clutch-ffi.md`](2026-09-07-test-built-filesystem-clutch-ffi.md).
It does not define packaged archives, signatures, dependency resolution,
cross-compilation, universal binaries, or ABI version 1.

## Migration

The implementation will replace `slug.io.fs.rust` with the native plugin
declaration and place a platform-specific test-built library under `native/`.
Repositories without a native plugin remain source-only. Existing direct Rust
host plugin registration stays available for embedding tests, but the CLI will
not silently substitute it for a declared native plugin.
