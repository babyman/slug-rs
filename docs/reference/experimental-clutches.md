# Experimental Clutches

## Status

This is the version-0 contract for a local, exploded-clutch experiment. It is
not source-language syntax, a released package format, or a compatibility
promise. It defines the smallest composition boundary that implementation work
may rely on. A later released clutch format requires a new decision record,
versioned schema, and regression coverage.

The source resolver, `$SLUG_HOME/clutch/manifest.toml` CLI discovery,
module-scoped Rust plugin initializer, and manifest-selected typed native
loader are implemented. `Vm::shutdown` and final loader drop clean plugin
state. Archive loading, package installation, and cross-platform binary
distribution remain unimplemented. The `slug.io.fs`, `slug.db.sqlite`,
`slug.random.secure`, and `slug.core` clutches bind plain C functions through
typed native bindings. The
SQLite clutch's Slug code over raw SQLite calls provides variadic SQL bindings,
row maps, prepared statements, and transactions. The bundled `slug.core` clutch
provides standard collections, standard input, and stateless math. Its
standard-input reader runs on a C thread and sends lines into a Slug channel
through a producer, under the `events` profile. This checkout
is a development repository: `make stage-native-clutches` builds every native
library for the current supported platform at its manifest-selected location
before local programs import it, generating and compiling the shim of each
typed clutch. Those staged libraries are ignored
local outputs rather than package artifacts. The staging script supports macOS,
Linux, and Windows hosts through POSIX-compatible Windows shells with a C
compiler and each adapter's development dependencies installed.

## Purpose and terms

A **module** is the Slug unit named by `import("name")`. A **clutch** is a
directory that distributes one or more modules and their optional supporting
native plugin. A **plugin** is a Slug-aware native implementation used only
behind a module's declared `foreign` interface.

Programs import modules; they do not import clutches or plugins. Version-0
Clutches supply source modules only, and exactly one provider owns a resolved
module. Initial whole-program `.cslug` artifacts are outside this boundary.

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
subset of its modules. Its `[native]` table uses
[typed native bindings](native-abi.md#typed-native-bindings), which bind the
clutch's `foreign` declarations to plain C functions:

```toml
[native]
profile = "core"
headers = ["native/source/slug_io_fs.h"]
sources = ["native/source/fs.c"]

[native.libraries]
"macos-aarch64" = "native/macos-aarch64/libslug_io_fs.dylib"

[native.resources."slug.io.fs".File]
type = "slug_fs_file"
release = { symbol = "slug_fs_close", signature = "(handle<File> file) -> void" }

[native.functions."slug.io.fs"]
readLineRaw = { symbol = "slug_fs_read_line", signature = "(handle<File> file, out cstr line) -> i32" }
```

The headers and sources are build inputs for the generated shim and are never
compiled by Slug at runtime. `native.libraries` selects one checked library
for the current supported OS/architecture; all paths must remain inside the
clutch. The library is loaded once for the clutch and may serve any of its
module identities. A module remains pure Slug unless it declares `foreign`
functions. The loader checks the library's shim table and every `foreign`
declaration in each of the clutch's modules against its binding before the
module is installed. It does not search system paths or fall back to a host
plugin. A C source that sends into a Slug channel includes the generated
`slug_producer.h` from beside its shim.

`link` names system libraries the shim links with `-l`. `native.link_args`
adds linker arguments for one platform: an argument starting with `-` is
passed as written, and any other is a path inside the clutch, such as a
vendored static library. `slug.arcade` uses it for its vendored raylib:

```toml
[native.link_args]
"macos-aarch64" = ["native/macos-aarch64/libraylib.a", "-Wl,-framework,Cocoa"]
```

Optionally, a clutch declares its test files, which a test runner finds
through the manifest rather than by listing directories:

```toml
[tests]
files = ["tests/string.slug", "tests/math.slug"]
```

Each path is relative to the clutch root, must stay within it, and must name a
`.slug` file. A test file's module identity is the clutch's name, its
directory name without `.clutch`, followed by the file's path without `.slug`,
with `/` written as `.`: `tests/string.slug` in `slug.core.clutch` is
`slug.core.tests.string`. It must be a valid module name and must not match a
module the repository provides. Test modules see their clutch's unexported
declarations through `describe`, so an indexed clutch's test modules resolve
only when the user running slug grants `describe.unexported` with
`--allow-unexported`; otherwise importing one reports that. A missing test
file is reported when its module is imported, so it never stops other
programs from loading the repository.

The experiment does not define archive encoding, signatures, remote fetching,
lock files, a package registry, dependency solving, or a `.cslug` entry.

### Assets

A clutch ships other files, such as scripts, images, or fonts, in an
`assets` folder at its root. There is no manifest entry: every file in the
folder is an asset, named by its path there written with `/`, such as
`htmx.js` or `images/logo.png`. A module whose source file is inside the
clutch embeds one with `asset("htmx.js")`, which the compiler reads through
the host's module resolver (`ModuleResolver::asset`) and stores as a bytes
constant; see [Assets](../language/language-specification.md#assets). Only
the files a program names are read, so a packaging tool learns which assets a
program uses by compiling it.

The desktop resolver reads an asset from the `assets` folder of the indexed
clutch whose directory holds the module's source. Each name segment must match
a directory entry exactly, case included; the file must be a regular file of at
most 16 MiB that stays inside the folder once links resolve. A static or nil
host provides no assets. `slug.net.http`'s `mediaType(name)` gives an asset's
media type from its extension when it is served.

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

The initial experiment loads only source modules. The adopted version-2
[compiled-module contract](compiled-modules.md) defines the future artifact
role; implementation remains separate work. Whole-program `.cslug` artifacts
do not participate in Clutch resolution or grant a fallback capability.

## Native plugins and lifecycle

A clutch plugin may provide only module-qualified registrations consumed by
the clutch's source `foreign` declarations. It cannot create ambient globals,
bind arbitrary C symbols, or alter Slug semantics. Foreign signature and
nominal-resource validation remain the native-ABI boundary.

For a clutch with a host plugin or native library, loading is transactional:

1. validate the selected clutch manifest and paths;
2. initialize the plugin once, or load the selected native library and
   validate its shim table, under a clutch-owned registration scope;
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
are recorded by the desktop host; one failure does not prevent other plugins
from being finalized.

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
