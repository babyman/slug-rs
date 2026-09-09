# Package the core standard library as a Clutch

## Context

`slug.std` was split between a source file under `lib/slug` and a static Rust
registration for `keys`. That made the standard module a special host feature
rather than a removable package and did not exercise the Clutch native boundary.

## Decision

`slug.core.clutch` is the bundled core standard-module package. Its initial and
only module is `slug.std`, whose public import identity remains unchanged. The
package owns the source declaration and native implementation of `keys`.

Prototype ABI 0.12 adds opaque callback-lifetime value tokens for reading map
entries and copying values into a list builder. It intentionally adds no
operation named after a standard-library function, no VM representation access,
no collection mutation, and no retained value handles.

## Consequences

A host installs `slug.std` by selecting `slug.core.clutch`; a missing package
produces the ordinary checked module-load failure. `slug.builtin` remains the
implicit host foundation. The source resolver preserves its existing precedence,
so project and library providers continue to win over a Clutch provider.

Native adapters must select ABI 0.12. The core adapter also proves ordered,
typed map-key transfer through the generic value-token operations.

## Migration

Existing Slug programs continue to use `import("slug.std")`. Development
checkouts must run `make stage-native-clutches` before executing a program that
imports it, just as for other native Clutches. ABI 0.11 native adapters are
rejected.
