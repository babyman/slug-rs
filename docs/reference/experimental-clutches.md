# Experimental Clutches

## Status

This is the version-0 contract for a local, exploded-clutch experiment. It is
not source-language syntax, a released package format, or a compatibility
promise. It defines the smallest composition boundary that implementation work
may rely on. A later released clutch format requires a new decision record,
versioned schema, and regression coverage.

The source resolver, `$SLUG_HOME/clutch/manifest.toml` CLI discovery,
module-scoped Rust plugin initializer, and manifest-selected version-0 native
loader are implemented. `Vm::shutdown` and final loader drop clean plugin
state. Archive loading, package installation, and cross-platform binary
distribution remain unimplemented. The local `slug.io.fs` clutch commits its
macOS ARM library as the first installed native-package experiment. The
`slug.db.sqlite` source clutch is built into a temporary installed layout by
the integration suite, proving variadic SQL bindings and compound value
transfer through an external C dependency. The stateless `slug.math` source
clutch provides the corresponding provider check without native resources or
an external library. This checkout
is a development repository: `make stage-native-clutches` builds every native
source adapter for the current supported platform at its manifest-selected
location before local programs import it. Those staged libraries are ignored
local outputs rather than package artifacts. The staging script supports macOS,
Linux, and Windows hosts through POSIX-compatible Windows shells with a C
compiler and each adapter's development dependencies installed.

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

Optionally, a clutch may provide one native implementation library for any
subset of its modules:

```toml
[modules]
"slug.arcade" = { source = "modules/arcade.slug" }
"slug.arcade.audio" = { source = "modules/audio.slug" }
"slug.arcade.input" = { source = "modules/input.slug" }

[native]
source = "native/source"
abi = "slug-ffi-prototype/0.13"

[native.libraries]
"macos-aarch64" = "native/macos-aarch64/libslug_io_fs.dylib"
```

The optional `native.source` directory contains FFI source or build inputs and
is never compiled by Slug at runtime. `native.libraries` selects one checked
library for the current supported OS/architecture; all paths must remain inside
the clutch. The library is initialized once for the clutch and may register
implementations for any of its module identities. A module remains pure Slug
unless its own `foreign` declarations require one of those registrations. The
loader accepts only `slug-ffi-prototype/0.13`, validates its library descriptor,
and
does not search system paths or fall back to a host plugin.

Native source may include `slug_ffi_helpers.h`, a private header-only SDK that
builds common asynchronous channel-source lifecycle patterns from the same
prototype ABI. It is optional convenience code, not a separate Clutch ABI or
runtime dependency.

The experiment does not define archive encoding, signatures, remote fetching,
lock files, a package registry, dependency solving, or a `.cslug` entry.

## Repository index and resolution

`$SLUG_HOME/clutch/manifest.toml` is the explicit local repository index. Its
`[modules]` table maps each exact import identity to a direct relative clutch
directory:

```toml
[modules]
"slug.io.fs" = "slug.io.fs.clutch"
```

Only clutches named by this file are importable. A target must remain inside
the repository, end in `.clutch`, and declare the same module identity in its
own `clutch.toml`. Rust hosts and tests may instead configure an explicit
repository index. This is local selection, not installation or dependency
solving. The loader resolves imports in this order:

1. the existing importer-relative, project-root, and library-root source
   providers, preserving current behavior;
2. a clutch-repository provider for the requested identity.

The index has one clutch entry per module identity. The clutch manifest must
repeat that identity in `[modules]`; disagreement, an absent file, or an
invalid manifest is a checked module-load error. The loader caches by module
identity and the selected provider, so a cyclic import still observes the
existing shared module-instance rules.

The initial experiment loads only source modules. A future artifact loader may
choose a compatible `.cslug` representation only under the independent
compiled-artifact contract; its absence or incompatibility must not silently
change the source module identity or grant a fallback capability.

## Native plugins and lifecycle

A clutch plugin may provide only module-qualified registrations consumed by
the clutch's source `foreign` declarations. It cannot create ambient globals,
bind arbitrary C symbols, or alter Slug semantics. Foreign signature and
nominal-resource validation remain the native-ABI boundary.

For a clutch with a host plugin or native library, loading is transactional:

1. validate the selected clutch manifest and paths;
2. initialize the plugin once, or load and validate the selected native
   descriptor, under a clutch-owned registration scope;
3. load requested source modules and validate each module's `foreign`
   declarations against that scope; and
4. publish the module and registrations only after all preceding steps pass.

On failure, the loader removes every registration from that scope and invokes
the plugin's cleanup hook. No partially initialized plugin may satisfy a later
import.

After a successful load, a plugin remains active for the VM lifetime. Resource
handles retain the owning module and resource-type identities already required
by the native ABI. VM shutdown is deterministic and ordered: it stops new VM
work, closes and destroys tracked resources into safe tombstones, destroys
native module state, removes registrations, runs each one-shot plugin cleanup,
and releases the final dynamic-library lease (`dlclose` or `FreeLibrary`).

Live unloading remains out of scope. The host must quiesce VMs sharing a loader
before shutdown. Native functions and resources retained by Rust values share
the plugin lifetime state; after shutdown they fail with
`native.plugin_inactive` and never call an unloaded pointer. Cleanup failures
are collected by `ModuleLoader::take_shutdown_errors`; one failure does not
prevent other plugins from being finalized.

## Required diagnostics and proof

Implementations must report checked module or native diagnostics, never host
panics, for: an unknown module, invalid manifest, an absent or unsupported
plugin or platform library, failed initialization, foreign mismatch, and
duplicate provider.
The first implementation must prove:

- a source-only clutch imports as an ordinary module;
- a native-backed clutch may serve a pure-Slug module and a module with
  declared foreign functions from the same clutch-owned plugin;
- cached and cyclic imports preserve existing module isolation and live
  bindings;
- failed initialization and foreign validation leave no registrations behind;
- shutdown deterministically finalizes plugin state and releases its library
  lease after its resources are destroyed.

## Relationship to other contracts

This document narrows the exploratory material in
[`../planning/completed/experimental-slug-clutches.md`](../planning/completed/experimental-slug-clutches.md).
It preserves the source module rules in
[`../language/language-specification.md`](../language/language-specification.md),
the native boundary in [`native-abi.md`](native-abi.md), and the future
compiled-artifact boundary in [`compiled-artifacts.md`](compiled-artifacts.md).
