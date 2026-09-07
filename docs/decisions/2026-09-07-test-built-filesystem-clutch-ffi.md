# Test-built filesystem clutch FFI

## Context

The `slug.io.fs` clutch proves source-module resolution, plugin registration,
nominal resources, and shutdown. The Rust plugin facade proved that flow but
did not exercise a dynamically loaded native descriptor. The existing
feature-gated C prototype lacked `nil` and text result operations, both needed
by `readLine(file):str|nil`.

## Decision

Keep dynamic filesystem loading test-only. The `ffi-prototype` feature extends
its unstable ABI-minor table with `set_nil` and copying `set_text` operations.
The filesystem C implementation lives in
`clutch/slug.io.fs.clutch/native/fs.c`; its integration test compiles it into a
temporary shared library, places that library inside a temporary exploded
clutch, and stages it through the ordinary clutch registrar.

The host copies text before the callback returns. The C module retains no
borrowed Slug data and keeps its resource implementation valid through the
existing resident-library rule. No dynamic library is committed, installed, or
selected by the CLI.

## Consequences

The regression proves a real dynamic descriptor can satisfy the canonical
filesystem declaration, including nominal `File` arguments/results, text and
nil results, resource destruction, and VM shutdown. The prototype remains
feature-gated and is not an ABI-v1 compatibility promise. A packaged clutch
binary layout, manifest library-selection field, and default CLI dynamic loader
remain separate decisions.

## Migration

None. Normal CLI filesystem support continues through the Rust test facade;
the dynamic implementation is exercised only by the feature-gated test.
