# Layered clutch repositories

## Status

Accepted (2026-11-15). Depends on
[project root marker](2026-11-15-project-root-marker.md).
Implementation: [project roots and layered clutches](../planning/completed/project-roots-and-layered-clutches.md).

## Context

The desktop runner indexes clutches from a single repository,
`$SLUG_HOME/clutch/manifest.toml` (`build_default_host_vm` in
`crates/slug/src/host.rs`). A project cannot provide its own clutch. It can
provide plain `.slug` modules from its root, but such a module has no assets
(see [clutch assets](2026-10-25-clutch-assets.md), whose consequences say
this needs a project manifest) and no native plugin.

The workaround is pointing `SLUG_HOME` at a directory inside the project.
That moves the library root and the library `slug.toml` too, and replaces the
installed clutches rather than adding to them, so the project manifest must
also list `slug.core` and every other clutch the program uses.

Application code ends up in the shared repository: `slug.games.crawl` and its
ten submodules are indexed in this checkout's `clutch/manifest.toml` beside
the standard clutches.

Configuration already layers this way: `$SLUG_HOME/lib/slug.toml`, then the
project `slug.toml`.

## Decision

The desktop runner indexes clutches from two layers, in this order:

| Layer | Repository manifest |
|---|---|
| home | `$SLUG_HOME/clutch/manifest.toml` |
| project | `<project root>/clutch/manifest.toml` |

The project root is the one the
[project root marker](2026-11-15-project-root-marker.md) selects. Either
layer may be absent. Each layer's manifest has the existing version-0
repository format and rules: entries name direct relative `.clutch`
directories inside that repository, and each clutch must declare the module
identities indexed to it.

- **One provider per identity, across layers.** A module identity or clutch
  test-module name indexed by both layers is a startup error. It names the
  identity and both repository manifests, with the clutch each one selects.
  A project cannot replace a home clutch, or a module of one.
- **Resolution order is unchanged.** Importer-relative, project-root, and
  library-root source still come before any clutch provider. A `.slug` file
  at the project root still takes priority over a clutch module of the same
  name, as now.
- **Nothing else depends on the layer.** Assets, native libraries, plugin
  activation, test modules, and diagnostics treat a project clutch exactly
  like a home clutch. Native library paths are relative to the clutch root,
  so they work in any layer.
- **One repository is read once.** When the project repository is the home
  one, as for a program whose project root is `$SLUG_HOME`, it is not
  layered over itself.
- **Hosts choose.** `ClutchRepository::layer` combines two repositories under
  the rule above, and also rejects a host plugin entry both define. Embedding hosts still assemble whatever
  repository they want. Only the CLI's default host reads the project layer.

## Consequences

A project can ship clutches with assets and native code next to its source,
and use the installed standard clutches without listing them. Crawl can index
its clutch in its own project and leave the shared repository to standard
library clutches.

Overriding a home clutch is intentionally unsupported. A native plugin
activates with its clutch's whole module set. Taking one module from the
project and its siblings from home would activate two copies of the same
library under different roots, which no lifecycle test covers. If overriding
is needed later, it should replace a whole clutch, and needs its own
decision.

Running a program loads native libraries its project indexes. That is not a
new capability, since project source can already use `slug.io.fs` and native
clutches from home, but the clutch reference should state it.

A `.cslug` artifact binds foreign functions from the clutches visible when it
runs. One that uses a project clutch must run where that project layer is
found. Otherwise its foreign module is not found when it starts, and the
error names that module.

`scripts/stage-native-clutches.sh` built a fixed list of this checkout's
clutches. It now takes `--repository DIR` and stages every clutch there with a
`[native]` table, deriving the shim provider from the clutch name
(`slug.io.fs` is `slug_io_fs`). A clutch that needs a prerequisite built
first provides `native/stage-prerequisites.sh`, as `slug.arcade` does for
its vendored raylib.

Documents to update when implemented: `reference/clutches.md`
(discovery, layering, the conflict error) and the clutch asset rule's
"needs a project manifest" note.

Tests: loader tests for two disjoint layers, an identity in both layers, a
test-module name in both layers, and assets from a project clutch; CLI tests
for a program importing a project clutch while `SLUG_HOME` provides
`slug.std`, and for a `tests/` program in the same project.

## Migration

None. With no project `clutch/manifest.toml`, indexing is unchanged. Moving
`slug.games.crawl` out of the shared repository is a separate change.
