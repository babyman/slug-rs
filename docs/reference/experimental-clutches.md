# Experimental Clutches

## Status

This is the version-0 contract for a local, exploded-clutch experiment. It is
not source-language syntax, a released package format, or a compatibility
promise. It defines the smallest composition boundary that implementation work
may rely on. A later released clutch format requires a new decision record,
versioned schema, and regression coverage.

The source resolver, `$SLUG_HOME/clutch` CLI discovery, and Rust-host-configured,
module-scoped plugin initializer are implemented. `Vm::shutdown` and final
loader drop clean plugin state. Archive loading, package installation, and
dynamic native loading remain unimplemented.

## Purpose and terms

A **module** is the Slug unit named by `import("name")`. A **clutch** is a
directory that distributes one or more modules and their optional supporting
native plugin. A **plugin** is a Slug-aware native implementation used only
behind a module's declared `foreign` interface.

Programs import modules; they do not import clutches or plugins. A module's
identity is independent of whether it is supplied as source now or a future
`.cslug` representation. Exactly one provider owns a resolved module.

## Local clutch identity

An exploded clutch is a directory ending in `.clutch` with a root
`clutch.toml`. Its version-0 manifest has this minimum shape:

```toml
[modules]
"slug.io.fs" = { source = "modules/fs.slug", plugin = "slug.io.fs.rust" }
```

Each key in `[modules]` is the exact module identity the clutch provides. The
source path is relative to the clutch root, must remain within it, and must
name a `.slug` file in version 0. A module without a plugin omits `plugin`.
The optional `plugin` value is an opaque host configuration key, not a
source-visible path or arbitrary native-library search instruction.

The experiment does not define archive encoding, signatures, remote fetching,
lock files, a package registry, dependency solving, or a `.cslug` entry.

## Resolution

The CLI discovers direct `.clutch` directories under `$SLUG_HOME/clutch` and
indexes every module identity in their manifests. Rust hosts and tests may also
configure an explicit repository. Directory scanning is local discovery, not
installation or dependency solving. The loader resolves imports in this order:

1. the existing importer-relative, project-root, and library-root source
   providers, preserving current behavior;
2. a clutch-repository provider for the requested identity.

The index must have at most one clutch entry per module identity. A manifest
must repeat that identity in `[modules]`; disagreement, an absent file, an
invalid manifest, or a duplicate manifest provider is a checked module-load
error. The loader caches by module identity and the selected provider, so a
cyclic import still observes the existing shared module-instance rules.

The initial experiment loads only source modules. A future artifact loader may
choose a compatible `.cslug` representation only under the independent
compiled-artifact contract; its absence or incompatibility must not silently
change the source module identity or grant a fallback capability.

## Native plugins and lifecycle

A clutch plugin may provide only module-qualified registrations consumed by
the clutch's source `foreign` declarations. It cannot create ambient globals,
bind arbitrary C symbols, or alter Slug semantics. Foreign signature and
nominal-resource validation remain the native-ABI boundary.

For a module that names a plugin, loading is transactional:

1. validate the selected clutch manifest and paths;
2. initialize the plugin under a clutch-owned registration scope;
3. compile the source module and validate its `foreign` declarations against
   that scope; and
4. publish the module and registrations only after all preceding steps pass.

On failure, the loader removes every registration from that scope and invokes
the plugin's cleanup hook. No partially initialized plugin may satisfy a later
import.

After a successful load, a plugin remains active for the VM lifetime. Resource
handles retain the owning module and resource-type identities already required
by the native ABI. On shutdown, the runtime rejects new calls, revokes producer
capabilities, requests resource closure, waits only for cooperative work to
quiesce, and destroys safe plugin state. It then removes plugin registrations.

This experiment deliberately does **not** call `dlclose` or its platform
equivalent. In-flight native work or leaked foreign references must keep native
code resident rather than permit use-after-unload. This follows the current
native ABI's process-lifetime code-residency rule; deterministic cleanup refers
to module state and registrations, not forced library-code unloading.

`Vm::shutdown` closes loader-tracked native resources before removing active
clutch registrations and running their cleanup hooks; subsequent execution on
that VM receives a checked `InvalidCall` error. Dropping the final
`ModuleLoader` remains a fallback that performs the same resource-first
cleanup. The host must quiesce other VMs sharing that loader before shutdown;
producer revocation and coordinated task cancellation remain future work.

## Required diagnostics and proof

Implementations must report checked module or native diagnostics, never host
panics, for: an unknown module, invalid manifest, an absent or unsupported
plugin, failed plugin initialization, foreign mismatch, and duplicate provider.
The first implementation must prove:

- a source-only clutch imports as an ordinary module;
- a native-backed clutch binds only its declared module-qualified foreign
  functions and nominal resources;
- cached and cyclic imports preserve existing module isolation and live
  bindings;
- failed initialization and foreign validation leave no registrations behind;
- shutdown cleans plugin state without unloading code still potentially in use.

## Relationship to other contracts

This document narrows the exploratory material in
[`../planning/experimental-slug-clutches.md`](../planning/experimental-slug-clutches.md).
It preserves the source module rules in
[`../language/language-specification.md`](../language/language-specification.md),
the native boundary in [`native-abi.md`](native-abi.md), and the future
compiled-artifact boundary in [`compiled-artifacts.md`](compiled-artifacts.md).
