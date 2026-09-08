# Make native library descriptors the sole lifecycle owner

## Context

The ABI 0.9 library descriptor made the loaded native library the practical
owner of native state and its teardown callback. Its nested module descriptor
still retained a `destroy_module` field, which the host intentionally ignored.
That redundant field suggested an unsupported per-module lifecycle and could
mislead native implementers into expecting a callback that would never run.

## Decision

Prototype ABI minor 10 removes the module-level teardown field and renames the
callback type to `slug_ffi_library_destroy_fn`. Only the library descriptor can
provide a teardown callback. The host calls it once after native resources have
been finalized and before releasing the loaded-library lease.

## Consequences

Module descriptors now contain only module identity, foreign functions, and
resource declarations. Native library state has one allocation and teardown
owner, which matches the host's shared FFI library state. This is a breaking
experimental ABI layout change.

## Migration

Native adapters, C fixtures, manifests, and the staged development binaries
move from `slug-ffi-prototype/0.9` to `slug-ffi-prototype/0.10`. The host does
not accept the ABI 0.9 module descriptor layout.
