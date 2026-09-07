# Clutch repository manifest

## Status

Accepted for the version-0 local experiment.

## Context

The initial CLI discovery scanned every `.clutch` directory directly beneath
`$SLUG_HOME/clutch`. That made directory contents silently affect the set of
importable modules and left no stable, reviewable installation index. The
experiment needs one explicit local boundary before it can grow toward a
packaged layout, without adding package installation or dependency solving.

## Decision

`$SLUG_HOME/clutch/manifest.toml` is the repository index. Its `[modules]`
table maps each import identity to one relative exploded-clutch directory:

```toml
[modules]
"slug.io.fs" = "slug.io.fs.clutch"
```

Only indexed clutches are candidates for resolution. Each target must be a
direct relative `.clutch` directory inside the repository, and its own
`clutch.toml` must declare the mapped module before it is indexed. The
per-clutch manifest remains authoritative for the source path and optional
plugin entry.

This narrows the repository-discovery portion of
[`2026-09-06-experimental-clutch-boundary.md`](2026-09-06-experimental-clutch-boundary.md).
It defines no archive format, binary selection, installation command, remote
registry, version selection, or dependency resolution.

## Consequences

Repository contents no longer implicitly publish modules. A repository can
contain staged or supporting clutch directories without exposing them, and the
index can intentionally map more than one module to one clutch. Invalid paths,
missing clutch directories, and disagreements between the repository and
clutch manifests are checked load errors.

Hosts that construct `ClutchRepository` directly remain supported for tests and
embedders; CLI discovery reads the repository manifest when it is present.

## Migration

The installed `slug.io.fs` clutch gains a repository entry. Local repositories
using direct directory discovery must add `manifest.toml` mappings for every
module they intend to expose.
