# Compiled module artifacts

## Context

Version-1 `.cslug` files are closed executable module graphs. They intentionally
do not consult source, Clutch, filesystem, or host resolution at load time.
That property is required for reproducible desktop execution and is the desired
firmware deployment boundary.

Projects and Clutches nevertheless need to distribute a module in compiled
form. A source module importing such a provider needs its public semantic
interface during compilation, while the final program needs to resolve imports
from a mixture of source, compiled modules, and target-native capabilities.

## Decision

Slug will define two roles in the version-2 `.cslug` family:

- a **program artifact** is a complete, closed executable graph with one root;
  execution resolves imports only from its embedded graph;
- a **module artifact** is one importable compiled module with a declared
  logical identity, public compiler interface, import requirements, and native
  capability requirements. It is a build input and cannot be run directly.

The build resolver selects exactly one provider for every import. A provider
may be source, a module artifact, or a Clutch-declared provider of either
kind. The selected providers are linked into one closed program artifact.

Native code is never encoded in `.cslug`. Module and program artifacts record
the native capability requirements implied by `foreign` declarations. Desktop
Clutches may satisfy those requirements through selected dynamic providers;
firmware profiles satisfy them only by statically linking the provider.

Version 1 remains unchanged. Version 2 has an explicit artifact role and a
canonical compiler-interface section; a v1 reader must reject v2 rather than
guess its meaning.

## Consequences

One compiler/linker model supports source projects, precompiled libraries,
Clutches, desktop execution, and firmware images. The ESP32-S3 build consumes
only a final closed program artifact embedded in flash; it needs no device
source resolver or module package loader.

The frontend must make its import snapshot serializable as a canonical public
interface. Resolver and Clutch manifests must identify the chosen provider and
native capability binding. Recompilation is the recovery path for interface or
VM compatibility mismatch; runtime execution never substitutes a provider.

## Migration

No v1 artifact changes role. Existing `slug compile` continues to produce a
v1 closed program until version 2 is implemented and exposed deliberately.
Existing source-only Clutches continue to resolve as they do today. Future
Clutch manifests opt into module-artifact providers explicitly.
