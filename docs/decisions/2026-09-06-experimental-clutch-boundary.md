---
title: ADR-000 Establish the experimental clutch boundary
date: 2026-09-06
---

## Status

Accepted for the version-0 local experiment.

## Context

Slug modules already have stable source-level identities, isolated instances,
and module-qualified foreign declarations. Source libraries, the future
`.cslug` format, and native extension loading need a common composition model,
but treating a file path or a dynamic library as an import target would expose
physical layout and unsafe loading policy to Slug programs.

The clutch exploration also proposed deterministic native-library unloading at
VM shutdown. That conflicts with the native ABI's safety rule that code remains
resident when outstanding native work may still reach it.

## Decision

A clutch is an experimental, host-resolved distributor of modules and optional
native plugins. Source code continues to import only module identities. Version
0 uses an exploded `.clutch` directory, a small `clutch.toml` manifest, and a
host-maintained one-provider-per-module index. Existing source resolution keeps
its current precedence; clutch resolution is an additional final provider.

The manifest records a publisher/name/version identity, host runtime range,
unstable plugin-facade marker, module-to-source mapping, and optional plugin
association. It supports `.slug` source modules only. It defines neither
package installation nor a published archive or `.cslug` format.

A clutch plugin registers capabilities transactionally and only for matching
module-qualified `foreign` declarations. Plugins stay active for the VM
lifetime. Shutdown deterministically releases registrations and safe plugin
state, but native library code remains resident for the process lifetime.

The detailed experimental contract is
[`../reference/experimental-clutches.md`](../reference/experimental-clutches.md).

## Consequences

### Positive

Module imports remain representation-independent and ordinary Slug libraries
need no dependency-management syntax. Native capabilities gain a single owner,
checked registration scope, and testable cleanup path.

### Negative

The initial experiment needs explicit repository-index configuration and cannot
claim package portability, dynamic ABI compatibility, hot unloading, or
artifact distribution.

### Neutral

Future `.cslug` selection remains governed by the compiled-artifact contract.
The existing static Rust facade and C prototype remain experimental and do not
become public ABIs.

## Migration

None. Existing imports, library roots, source modules, and native registrations
remain unchanged until an opt-in implementation is added.
