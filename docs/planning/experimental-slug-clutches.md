# Experimental: Slug Clutches

## Status

**Experimental architectural exploration.**

This document explores a possible evolution of Slug's module and distribution model through a new concept: the
**clutch**.

A clutch is a composed, potentially distributable unit containing Slug modules and the supporting artifacts required to
provide them.

The name is intentionally Slug-specific: slugs lay eggs in clutches, while the ordinary meaning of *clutch* also
describes a related group held together as a unit.

This is not a language specification or implementation commitment.

The immediate goal is deliberately smaller: determine whether the clutch and runtime-plugin abstractions simplify Slug's
module, FFI, and runtime architecture without introducing dependency-management ceremony into Slug programs.

Existing module and import behaviour remains authoritative until the experiment proves otherwise.

The first experiment's selected local manifest, resolver, and plugin-lifecycle
rules are recorded in
[`../reference/experimental-clutches.md`](../reference/experimental-clutches.md).
That version-0 contract narrows this exploration where they differ, notably by
retaining native library code for the process lifetime rather than requiring
`dlclose` during VM shutdown.

---

## Terminology

This experiment separates three concepts:

```text
module
    A Slug language/compilation unit.

clutch
    A composed and potentially distributable
    collection of modules and supporting artifacts.

plugin
    A native component that extends VM/runtime capability.
```

The corresponding artifacts are:

```text
.slug       source representation of a Slug module
.cslug      future compiled representation of a Slug module
.clutch     composed Slug library/application package
```

At present, `.cslug` bytecode is **conceptual rather than implemented**.

The clutch design should accommodate compiled modules in the future, but the initial experiment must not depend upon
them.

A module belongs to the language.

A clutch belongs to composition and distribution.

A plugin belongs to the runtime.

The central relationship is:

> **Programs import modules. Clutches distribute them. Plugins implement runtime capabilities they may require.**

---

# Modules

A module remains the unit visible to Slug source code.

For example:

```slug
val json = import("slug.json")
val fs = import("slug.io.fs")
```

An import asks for a module by identity.

It should not fundamentally mean:

```text
load the file named slug/io/fs.slug
```

Instead:

> **`import("slug.io.fs")` means "give me the module named `slug.io.fs`."**

How that module is provided is a runtime concern.

---

## Module representations

Today, a module is represented by Slug source:

```text
fs.slug
```

A future compiled representation may be:

```text
fs.cslug
```

Conceptually these would represent the same module:

```text
              slug.io.fs
                  │
          ┌───────┴───────┐
          │               │
       fs.slug          fs.cslug
       source           compiled
                         future
```

The distinction matters to the compiler and runtime but should generally not matter to importing Slug code.

This gives us an important principle:

> **Module identity should not depend upon its physical representation.**

For the initial clutch experiment, however, modules should remain `.slug` source modules.

---

## Future compiled-module compatibility

If `.cslug` is implemented later and distributed inside clutches, a compiled module will need to identify the
bytecode/runtime contract against which it was compiled.

The exact compatibility mechanism belongs to the `.cslug` design rather than the clutch design.

Conceptually, a future compiled module may carry information such as:

```text
module: slug.io.fs
format: cslug
bytecode: 3
```

A future loader could then prefer compatible compiled code and fall back to source when available.

This behaviour is **not part of the initial clutch experiment** because `.cslug` bytecode does not yet exist.

The clutch abstraction should simply avoid making future compiled-module support difficult.

---

# Clutches

A clutch is the unit of **composition and distribution**.

A clutch provides one or more Slug modules and may carry everything required to make those modules work.

Conceptually:

```text
Clutch
  │
  ├── Slug modules
  │     ├── .slug
  │     └── future .cslug
  │
  ├── native runtime plugins
  ├── resources
  └── composition metadata
```

A clutch therefore distributes modules rather than source files.

For the initial experiment, a clutch may simply contain:

```text
slug.io.fs.clutch/
    modules/
        fs.slug
```

The design should remain compatible with a future release clutch containing:

```text
slug.io.fs.clutch
    modules/
        fs.cslug
```

without making compiled modules a prerequisite today.

---

## One clutch, several modules

A clutch may naturally provide several related modules.

For example:

```text
sqlite.clutch
    modules/
        sqlite.slug
        query.slug
        migration.slug
```

might advertise:

```text
slug.sqlite
slug.sqlite.query
slug.sqlite.migration
```

Slug programs still import the individual modules:

```slug
val sqlite = import("slug.sqlite")
val migration = import("slug.sqlite.migration")
```

The fact that both happen to be distributed by the same clutch is not part of their source-level interface.

A useful initial invariant is:

> **At runtime, exactly one provider owns a resolved module.**

Allowing several clutches to contribute independently to a single module would substantially complicate module ownership
and resolution and is not part of this experiment.

---

# Runtime plugins

A plugin is a native component extending VM/runtime capability.

For example:

```text
slug.io.fs.clutch
       │
       ├── slug.io.fs module
       │
       └── filesystem plugin
```

The three layers are:

```text
slug.io.fs.clutch          distribution/composition
        │
        ▼
slug.io.fs module          Slug API
        │
        ▼
filesystem plugin          VM/native implementation
```

Most clutches should require no plugin.

A completely Slug-implemented library might simply be:

```text
slug.template.clutch
    parser.slug
    template.slug
    renderer.slug
```

Native functionality is optional rather than intrinsic to clutches.

---

# Plugin ownership and lifecycle

Plugin lifecycle is part of the architecture and should be tested explicitly.

A plugin should not be treated as a loose collection of globally registered native functions with no owner.

Loading a clutch should establish an ownership relationship between the clutch and everything its plugin registers.

Conceptually:

```text
LoadedClutch
    │
    └── PluginHandle
          ├── foreign functions
          ├── runtime resource types
          ├── capability registrations
          ├── plugin-owned state
          ├── native resources
          └── cleanup actions
```

The exact Rust representation is an implementation detail.

The important property is:

> **Everything introduced by a plugin should have a defined owner and lifetime.**

This applies even if Slug ultimately decides not to support arbitrary runtime plugin unloading.

---

## Loading

When a clutch requiring a plugin is first resolved, the runtime may perform a sequence such as:

```text
resolve module
    ↓
open clutch
    ↓
load native plugin
    ↓
create PluginHandle
    ↓
register plugin-owned capabilities
    ↓
validate Slug foreign declarations
    ↓
load module
    ↓
make module available
```

If any step fails, the partially established plugin should be unwound cleanly.

For example, a failed declaration validation should not leave half-registered filesystem functions in the VM.

Loading therefore needs transactional behaviour at the plugin boundary even if the internal implementation is simple.

---

## Runtime ownership

While a plugin is active, registrations should remain associated with the plugin that created them.

For example:

```text
filesystem plugin
    owns:
        File resource implementation
        open
        read
        close
        filesystem state
```

This gives the runtime a concrete answer to questions such as:

* who registered this foreign function?
* who owns this runtime resource implementation?
* which clutch caused this capability to exist?
* what must be cleaned up when the runtime shuts down?

This ownership information is also useful for diagnostics and inspection.

---

## Resource lifetime

Typed resources make plugin lifecycle especially important.

For example:

```slug
export resource File
```

may correspond to live native file handles owned by the filesystem plugin.

The plugin cannot safely disappear while instances of that resource remain alive.

A minimum invariant should therefore be:

> **A plugin must remain valid for at least as long as any runtime value whose implementation depends upon it.**

This suggests that plugin-owned resource types and resource instances must contribute to plugin lifetime or otherwise
prevent unsafe teardown.

The experiment should make this relationship explicit.

---

## Plugin unloading and shutdown

The initial plugin model does **not** require live unloading while the VM is running.

A plugin may remain loaded for the full active lifetime of the VM.

However, deterministic plugin-state cleanup at VM shutdown **is required
behaviour**. Native library-code unloading is not.

The minimum lifecycle is:

```text
plugin load
    ↓
plugin active
    ↓
VM shutdown begins
    ↓
quiesce plugin
    ↓
release plugin-owned resources
    ↓
run plugin shutdown hook
    ↓
unregister plugin-owned capabilities
    ↓
retain native library code for process lifetime
```

This distinction is important:

```text
live unloading
    optional / future

shutdown state cleanup
    required
```

A VM-lifetime plugin therefore means:

> **The plugin remains active while the VM is active; shutdown deterministically
> cleans its state and registrations without unloading code still potentially in use.**

Shutdown must not rely solely on process termination to reclaim plugin state.

### Shutdown ordering

VM shutdown should proceed in an order that preserves plugin safety.

At minimum:

1. prevent new work from being started through the plugin;
2. complete or cancel runtime work that depends upon the plugin;
3. finalize or release plugin-owned resource instances;
4. invoke the plugin's shutdown/cleanup hook;
5. unregister functions, resource implementations, and capabilities owned by the plugin;
6. unload the native library.

For `slug.io.fs`, this means live `File` resources must not outlive the filesystem plugin implementation they depend upon.

Conceptually:

```text
File resources
      │
      ▼
filesystem plugin
      │
      ▼
native library
```

Teardown therefore occurs in the reverse direction:

```text
release File resources
        ↓
shutdown filesystem plugin
        ↓
unregister filesystem capability
        ↓
unload native library
```

### Shutdown guarantees

The first experiment should require the following guarantees:

* every loaded plugin receives exactly one shutdown opportunity;
* plugin shutdown occurs before its native library is unloaded;
* plugin-owned resources are finalized before the plugin implementation becomes invalid;
* plugin registrations are removed during shutdown;
* partial plugin initialization is unwound if loading fails;
* plugin shutdown is deterministic and testable;
* plugin teardown does not depend on the operating system reclaiming process memory.

A plugin shutdown failure should be reported, but should not prevent the VM from attempting to clean up other loaded plugins.

### Live unloading

Live unloading remains explicitly out of scope.

The ownership model should avoid making it impossible later, but the first implementation does not need to support:

```text
slug.io.fs active
      ↓
unload plugin
      ↓
continue VM execution
```

That introduces substantially harder questions around live resources, callbacks, dependencies, outstanding work, and module references.

Those questions should only be addressed if a concrete use case appears.

For the current experiment, the lifecycle contract is simpler:

> **Plugins load on demand, remain active for the VM lifetime, and unload deterministically during VM shutdown.**

---

## Plugin failure and cleanup

Plugin cleanup should be deterministic at least in these cases:

```text
plugin initialization failure
clutch loading failure
VM shutdown
```

A plugin should have an opportunity to release plugin-global state and native resources.

The experiment should specifically verify that:

1. failed initialization leaves no registrations behind;
2. failed module validation leaves no registrations behind;
3. VM shutdown releases plugin-global state;
4. live `File` resources are safely finalized before or during plugin teardown;
5. plugin teardown cannot invalidate still-live resource values.

These lifecycle behaviours are more important to the first experiment than hot unloading.

---

# The VM boundary

This experiment does **not** imply that Slug itself should become a collection of interchangeable plugins.

The VM continues to define fundamental Slug semantics.

Likely VM responsibilities include:

```text
value representation
bytecode execution
functions and call frames
memory management
errors
module loading
native call boundary
runtime type/resource support
plugin/capability machinery
plugin ownership/lifecycle tracking
```

Plugins may extend what the VM can **do**, but should not redefine what Slug **means**.

Reasonable plugin-provided capabilities might include:

```text
filesystem
networking
databases
cryptography
timers
channels
```

Plugins should not redefine:

```text
+
function calls
match
lists
structs
numeric semantics
```

The exact location of concurrency remains deliberately unresolved.

Concurrency may ultimately require deeper VM support than ordinary runtime capabilities. The plugin experiment should
provide evidence for that decision rather than forcing it prematurely.

---

## Bigger and smaller at the same time

A plugin architecture potentially allows Slug to become both **larger and smaller**.

The ecosystem can grow:

```text
Slug
├── filesystem
├── networking
├── databases
├── graphics
├── crypto
├── concurrency
└── ...
```

while an individual runtime only needs the capabilities actually reachable by its program.

For example, if a program never imports:

```slug
import("slug.channel")
```

then a target build may have no reason to include the channel module, channel plugin, or capabilities used exclusively
by channels.

Imports therefore become a natural description of required capability.

This is particularly attractive for constrained targets.

An ESP32 build should ideally still be **Slug**, not an "ESP32 Slug" dialect.

The target simply provides a smaller set of modules and runtime capabilities.

A useful principle is:

> **A target does not define a reduced Slug language. It defines which module and runtime capability providers are
available.**

---

# Native code behind modules

FFI remains an implementation mechanism behind the module interface.

For example, `slug.io.fs` might declare:

```slug
export resource File

export foreign open(path:str):File
export foreign read(file:File):bytes
export foreign close(file:File):nil
```

A filesystem plugin supplies those implementations.

The consuming program only sees:

```slug
val fs = import("slug.io.fs")
```

The clutch owns the relationship between the Slug module and its native implementation.

During loading, the runtime can validate that the plugin satisfies the module's typed foreign declarations before
exposing the module.

Thus:

> **FFI is an implementation mechanism belonging behind the module interface. The clutch carries that implementation.**

---

# Installed clutch repository

Clutches suggest a simple installed-library repository.

Conceptually:

```text
Slug repository
    │
    ├── clutches/
    │     ├── sqlite.clutch
    │     ├── io-fs.clutch
    │     └── arcade.clutch
    │
    └── manifest.toml
```

The repository manifest maintains a mapping such as:

```text
slug.sqlite          → sqlite.clutch
slug.io.fs           → io-fs.clutch
slug.arcade          → arcade.clutch
slug.arcade.audio    → arcade.clutch
```

Fundamentally, this is:

```text
module identity → provider
```

Installation can establish this mapping once rather than requiring the runtime to scan every clutch whenever an import
occurs.

---

## Installation versus runtime resolution

Conceptually:

```text
INSTALL TIME

io-fs.clutch
      │
      ▼
inspect clutch
      │
      ├── validate metadata
      ├── discover provided modules
      ├── validate native artifacts
      └── update repository manifest
                    │
                    ▼
        slug.io.fs → io-fs.clutch
```

Runtime resolution then becomes:

```text
import("slug.io.fs")
        │
        ▼
resolve module
        │
        ▼
repository lookup
        │
        ▼
io-fs.clutch
        │
        ├── establish plugin
        ├── validate foreign declarations
        ├── load module source
        └── expose module
```

The runtime does not need to discover which package might provide the requested module.

The repository already knows.

---

# Import semantics

Today import can approximately be thought of as:

```text
import("foo")
      │
      ▼
resolve foo.slug
      │
      ▼
load module
```

The generalized model becomes:

```text
import("foo")
      │
      ▼
resolve module foo
      │
      ├── already loaded?
      ├── local/source provider?
      └── installed clutch provider?
                    │
                    ▼
                 clutch
                    │
                    ├── establish capabilities
                    ├── load native plugin if required
                    ├── validate foreign declarations
                    └── expose module
```

This requires no dependency declaration in ordinary Slug source beyond the imports the program already contains.

No POM-shaped dependency description is required merely to tell the runtime what the source has already told it.

---

## Module resolution as an internal abstraction

Clutch support may eventually justify making module providers explicit internally.

Conceptually:

```rust
trait ModuleResolver {
    fn resolve(&self, name: &ModuleName) -> Option<ModuleProvider>;
}
```

with possible providers such as:

```rust
enum ModuleProvider {
    Source(PathBuf),
    Clutch(ClutchId),

    // Future:
    Compiled(PathBuf),
}
```

This is illustrative rather than a proposed API.

The important property is that a clutch becomes another provider of modules rather than a special case spread throughout
the implementation of `import`.

---

# Clutch artifact format

If the experiment succeeds, the canonical `.clutch` artifact will be a **ZIP archive** containing a defined Slug clutch
layout.

ZIP is deliberately conventional. Slug does not need a custom archive format.

A future packaged clutch may look like:

```text
slug.io.fs.clutch
│
├── clutch.toml
│
├── modules/
│   └── fs.slug
│
├── native/
│   ├── macos-aarch64/
│   │   └── slug_io_fs.dylib
│   ├── linux-x86_64/
│   │   └── slug_io_fs.so
│   └── windows-x86_64/
│       └── slug_io_fs.dll
│
└── resources/
```

Once `.cslug` exists, compiled modules may also be carried under `modules/`.

`clutch.toml` lives at the root of the archive and acts as the well-known entry point describing its contents.

The exact TOML schema is deliberately **not** defined by this experiment.

The implementation should first determine which metadata is genuinely required.

---

## TOML configuration

Slug-owned configuration and manifest files should use **TOML**, consistent with the existing Slug convention.

This includes clutch metadata:

```text
clutch.toml
```

and repository metadata:

```text
manifest.toml
```

A future clutch manifest might conceptually contain information such as:

```toml
[clutch]
name = "slug.io.fs"
version = "0.1.0"

[modules]
"slug.io.fs" = "modules/fs.slug"

[native]
plugin = "slug_io_fs"
```

This is illustrative only.

The experiment should avoid prematurely designing a comprehensive package metadata schema.

---

# Exploded clutches during development

The first implementation does **not** need to implement ZIP packaging.

During clutch development, a `.clutch` may be represented as an **exploded directory** using exactly the same logical
layout as the future ZIP artifact.

For example:

```text
slug.io.fs.clutch/
├── clutch.toml
├── modules/
│   └── fs.slug
└── native/
    └── <current-platform>/
        └── filesystem plugin
```

The important rule is:

> **An exploded clutch and a packaged clutch have the same logical contents and semantics; only their storage
representation differs.**

This allows the first experiment to concentrate on module resolution, plugin loading, resource types, FFI validation,
ownership, and lifecycle rather than ZIP implementation.

A future packaging step becomes conceptually:

```text
slug.io.fs.clutch/
        │
        │ ZIP
        ▼
slug.io.fs.clutch
```

No clutch semantics change.

---

## Clutch storage abstraction

The implementation should avoid coupling clutch loading directly to directories.

Conceptually, a small internal abstraction could allow the loader to consume different representations:

```text
ClutchSource
    │
    ├── DirectoryClutch
    ├── ZipClutch
    └── EmbeddedClutch
```

The exact Rust interface is an implementation detail.

The important point is that the module/plugin loader operates on the **logical contents of a clutch**, not on
assumptions about how those contents are physically stored.

This also leaves a natural path toward clutches embedded in standalone executables or firmware.

---

# Executable clutches

Although not required for the initial experiment, the clutch abstraction naturally extends to applications.

A future executable clutch could contain:

```text
myapp.clutch
├── clutch.toml
├── app/
│   └── main.slug or future main.cslug
├── modules/
│   ├── slug.json.slug
│   └── slug.sqlite.slug
├── native/
│   └── ...
└── resources/
```

Its manifest could identify an entry module.

Then:

```text
slug run myapp.clutch
```

could establish the clutch's embedded module repository, load its required capabilities, resolve the entry module, and
invoke `main`.

The important property is that imports inside the application remain ordinary Slug imports.

---

## Closing the dependency graph

A future application build could start at its entry module, follow its imports and runtime requirements, and freeze the
resulting dependency closure into an executable clutch.

Conceptually:

```text
application modules
       │
       ▼
resolve imports
       │
       ▼
resolve clutch providers
       │
       ▼
resolve plugin capabilities
       │
       ▼
closed dependency graph
       │
       ▼
application.clutch
```

Once `.cslug` exists, that closed graph may naturally contain compiled modules.

This is a future deployment direction rather than something the first filesystem experiment needs to prove.

---

# Standalone and embedded builds

A closed executable clutch also creates a possible path toward standalone applications:

```text
Slug VM
+
embedded application.clutch
=
standalone executable
```

The `.clutch` remains the deployment/composition artifact.

The standalone executable merely embeds it with an appropriate runtime.

The same mechanism could eventually be useful for constrained targets such as ESP32.

A target build would include only the modules and capabilities reachable from the application.

That is a future consequence of the architecture, not part of the initial plugin test.

---

# Machine-readable inspection

Clutches also create an opportunity for tooling and AI agents to inspect software composition without executing it.

For example:

```text
slug clutch describe io-fs --json
```

might report:

```json
{
  "name": "slug.io.fs",
  "modules": [
    "slug.io.fs"
  ],
  "native": true,
  "lifecycle": "vm"
}
```

while:

```text
slug module describe slug.io.fs --json
```

could describe the actual Slug interface.

This creates a useful distinction:

```text
clutch.toml
    describes composition

module/type information
    describes the Slug API

repository manifest
    describes installed providers

plugin ownership state
    describes active runtime capability
```

Native-containing clutches should be clearly identifiable without executing their native code.

---

# Structured resolution and lifecycle errors

Because resolution and lifecycle are explicit, failures can also be explicit and machine-readable.

For example:

```json
{
  "error": "module_not_found",
  "module": "slug.io.fs",
  "searched": [
    "local",
    "installed_clutches"
  ]
}
```

or:

```json
{
  "error": "plugin_initialization_failed",
  "module": "slug.io.fs",
  "clutch": "slug.io.fs",
  "plugin": "slug_io_fs"
}
```

A future unload attempt might produce:

```json
{
  "error": "plugin_in_use",
  "plugin": "slug_io_fs",
  "liveResources": 3
}
```

even if explicit unloading is not initially exposed.

This fits naturally with Slug's broader direction toward structured runtime diagnostics suitable for both humans and
agents.

---

# What this experiment deliberately does not solve

The clutch experiment does **not** currently propose:

* a public package registry;
* remote dependency resolution;
* a dependency version solver;
* lock files;
* signing infrastructure;
* automatic updates;
* a finalized `clutch.toml` schema;
* ZIP reading/writing;
* implemented `.cslug` bytecode;
* compiled-module compatibility rules;
* standalone executable generation;
* ESP32 tooling;
* arbitrary plugin hot unloading;
* the final concurrency/runtime boundary.

These are possible consequences of the architecture, not prerequisites for proving it.

The immediate experiment should remain small.

---

# First experiment: `slug.io.fs`

`slug.io.fs` is the preferred first clutch/plugin experiment.

It exercises the important architectural boundaries without introducing the unresolved concurrency question.

A deliberately small Slug API could be sufficient:

```slug
export resource File

export foreign open(path:str):File
export foreign read(file:File):bytes
export foreign close(file:File):nil
```

The experimental exploded clutch should use source modules because `.cslug` is not yet implemented:

```text
slug.io.fs.clutch/
├── clutch.toml
├── modules/
│   └── fs.slug
└── native/
    └── <current-platform>/
        └── filesystem plugin
```

The complete experimental path is:

```text
import("slug.io.fs")
       │
       ▼
module resolver
       │
       ▼
repository manifest
       │
       ▼
slug.io.fs.clutch/
       │
       ├── clutch.toml
       ├── fs.slug
       └── filesystem plugin
                  │
                  ▼
              Slug VM
```

The experiment should answer:

1. Can `import("slug.io.fs")` treat a clutch-provided module exactly like an ordinary Slug module?
2. Can the repository map the module identity to its clutch provider?
3. Can the clutch establish a native plugin?
4. Can the plugin provide a typed `File` resource?
5. Can the runtime validate foreign declarations against the plugin implementation?
6. Can native errors cross the boundary cleanly?
7. Does every plugin registration have a clear owning `PluginHandle` or equivalent?
8. Does failed plugin initialization leave the VM unchanged?
9. Does failed foreign-declaration validation unwind plugin registrations cleanly?
10. Can live `File` resources safely keep their plugin implementation valid?
11. Can VM shutdown deterministically clean up plugin-owned resources and plugin-global state?
12. Can the implementation support VM-lifetime plugins without baking in assumptions that make future unloading
    impossible?
13. Does removing the clutch completely remove filesystem capability without changing Slug language semantics?
14. Does the design simplify the VM and FFI rather than merely moving their complexity elsewhere?

The key success criteria are:

> **After extracting `slug.io.fs`, removing its clutch should make filesystem capability cease to exist without
requiring changes to the Slug VM or language.**

and:

> **Loading, failure, resource lifetime, and shutdown must all have explicit plugin ownership semantics.**

The initial implementation should use an exploded `.clutch` directory and `.slug` modules.

ZIP packaging and `.cslug` support come later.

---

# Future validation

The filesystem experiment proves the mechanism once.

A useful later sequence would be:

```text
slug.io.fs
    proves native runtime capability,
    typed resources, and lifecycle ownership

slug.sqlite
    proves external native dependencies
    and a richer foreign API

third capability
    tests whether the abstraction
    is genuinely reusable
```

Only after several substantially different implementations should the plugin/clutch interfaces be considered stable.

---

# Working hypothesis

The hypothesis behind this experiment is:

> **A Slug module remains the unit of Slug code and API, while a clutch becomes the unit that composes and distributes
modules and their supporting capabilities.**

Today, modules are represented by `.slug` source.

A future `.cslug` representation should fit the same model without changing module identity or clutch semantics.

A clutch may distribute modules together with native plugins and resources.

An installed clutch repository maps module identities to their providers.

Plugins have explicit ownership and lifecycle even if they initially remain loaded for the lifetime of the VM.

An executable clutch may eventually close the complete dependency graph of an application into a shippable artifact.

A target build may eventually combine that closed graph with only the runtime capabilities required for the target.

The architecture can therefore be summarized as:

```text
                    Slug program
                         │
                 import("slug.io.fs")
                         │
                         ▼
                  Module Resolver
                         │
              ┌──────────┴──────────┐
              │                     │
         local module        repository index
                                    │
                                    ▼
                            slug.io.fs.clutch
                                    │
                       ┌────────────┴────────────┐
                       │                         │
                   fs.slug                  fs plugin
                                              │
                                      explicit ownership
                                      and lifecycle
                                              │
                                              ▼
                                           VM core
```

And more succinctly:

> **The VM implements computation.**
> **Plugins implement capability.**
> **Modules expose capability.**
> **Clutches distribute capability.**
> **The repository resolves capability.**
> **Plugin ownership governs capability lifetime.**

For now, **clutch** remains an experimental architectural concept.

The next step is to make `slug.io.fs` prove it.

---

# Actionable execution plan

The version-0 contract in
[`../reference/experimental-clutches.md`](../reference/experimental-clutches.md)
defines the work below. Each phase is independently reviewable. Do not begin a
later phase until the prior phase's exit criteria hold.

## Phase 3 — Source-only clutch resolution

**Goal:** make an exploded clutch another provider of an ordinary `.slug`
module, with no native loading.

### Todo

- [x] Keep the existing Cargo feature set unchanged. The dedicated experiment
  branch is the isolation boundary; preserve existing CLI and
  `ModuleLoader::new` behavior until a clutch repository is explicitly
  configured.
- [x] Add private clutch manifest types and parsing in a dedicated module (for
  example `src/clutch.rs`) using the existing TOML dependency.
- [x] Validate `format = 0`, required clutch identity fields, runtime and
  plugin-facade requirements, module names, and source paths before opening a
  module. Reject paths outside the clutch root, missing files, unknown keys
  that would change behavior, and duplicate module providers.
- [x] Add an explicit, test-configured clutch repository index from module
  identity to exploded-clutch directory. Do not add installation commands,
  repository scanning, or CLI flags yet.
- [x] Extend `src/module.rs` module resolution so the
  existing importer-relative, project-root, and library-root lookup remains
  first and clutch lookup is the final provider.
- [x] Cache a clutch-provided module under the same module identity and
  initialization rules as an ordinary source module; do not create a second
  import namespace.
- [x] Add focused `tests/module_loader.rs` coverage for source-only success,
  existing-provider precedence, a missing indexed clutch, malformed manifests,
  invalid paths, requirements failure, duplicate providers, and cyclic imports.
- [x] Update the reference contract only if implementation reveals an
  unspecified observable outcome; otherwise keep this phase implementation-only.

### Exit criteria

- `import("example.library")` loads a source module from an indexed exploded
  clutch with the same exports, isolation, live bindings, and error category as
  an ordinary module.
- All invalid inputs return checked module-load errors and leave the loader
  cache unchanged.
- `cargo test --features metrics --test module_loader` and `make check` pass.

## Phase 4 — Plugin-owned foreign registration

**Goal:** let a clutch plugin supply only the native declarations of its own
module, without ambient registrations or partial state.

### Todo

- [ ] Model a loader-private plugin registration scope with its clutch and
  module identities, registered foreign functions, resource types, cleanup
  hook, and active/failed state.
- [ ] Extend the existing foreign registry in `src/module.rs` so a scope can
  stage a batch of registrations, validate uniqueness, and atomically publish
  or discard the batch. Preserve ordinary host registrations for builtins and
  non-clutch use.
- [ ] Define a narrow Rust-only v0 plugin initializer that receives the scoped
  registrar; it must not receive VM internals, globals, scheduler access, or
  arbitrary source values.
- [ ] Resolve the manifest's plugin entry through host-controlled test
  configuration. Do not teach the clutch manifest how to search arbitrary
  dynamic libraries.
- [ ] Make module loading follow the contract order: manifest validation,
  scoped initialization, compile/foreign validation, then publication.
- [ ] Ensure every failure path invokes cleanup once, removes staged
  registrations, and permits a later clean retry.
- [ ] Add VM and module-loader tests for matching foreign functions, resource
  type ownership, initializer failure, missing registration, arity/signature
  mismatch, and no-registration-leak retry behavior.

### Exit criteria

- A plugin can satisfy `foreign` declarations only for the module selected
  from its clutch; it cannot register an unrelated capability.
- Failed initialization and failed declaration validation are observationally
  equivalent to never having loaded the plugin.
- `make test-vm`, `cargo test --features metrics --test module_loader`, and
  `make check` pass.

## Phase 5 — `slug.io.fs` vertical slice and lifecycle proof

**Goal:** prove the complete source-module, plugin, foreign-resource, and
shutdown flow with one small filesystem capability.

### Todo

- [ ] Move or recreate the minimal `slug.io.fs` declaration module inside an
  exploded test clutch: `File`, `open`, `read`, and idempotent `close`.
- [ ] Implement a test-only Rust v0 filesystem plugin first. Reuse existing
  nominal resource and foreign-call validation rather than creating a clutch
  resource representation.
- [ ] Use a consumer fixture that imports only `slug.io.fs`; it must not know
  the clutch path, plugin name, or native implementation.
- [ ] Prove resource cleanup through explicit close, error unwinding, and VM
  shutdown. Confirm shutdown rejects new calls and releases registrations and
  plugin-owned state.
- [ ] Prove that code residency is not mistaken for state residency: cleanup
  runs deterministically, but no test expects `dlclose`.
- [ ] Add diagnostic assertions for bad manifest data, unsupported platform,
  plugin initialization failure, foreign mismatch, and an unavailable module.
- [ ] Add the relevant support-matrix/README wording only if the feature
  becomes user-invokable; otherwise retain its test-only experimental status.

### Exit criteria

- The vertical slice works through `import("slug.io.fs")` alone and preserves
  nominal `fs.File` checks across module boundaries.
- Cleanup and every failure mode are covered by checked-error regressions.
- Module-loader and VM tests, then `make check`, pass on the supported host
  platforms.

## Phase 6 — Separate artifact and dynamic-loader decisions

**Goal:** decide whether the proven local model justifies portable artifacts or
dynamic native loading; do not implement either by implication.

### Todo

- [ ] Review Phase 5 evidence against the `.cslug` implementation gate in
  [`../reference/compiled-artifacts.md`](../reference/compiled-artifacts.md).
  If it is not met, retain `.slug`-only clutches.
- [ ] If compiled modules proceed, write the complete `.cslug` version-1
  schema, verifier rules, compatibility negotiation, malformed-input fixtures,
  and loader tests before adding a manifest representation entry.
- [ ] Review dynamic plugin loading against the native-ABI implementation gate
  in [`../reference/native-abi.md`](../reference/native-abi.md). Publish C
  declarations and ABI conformance tests before any released loader support.
- [ ] Keep the existing `ffi-prototype` feature isolated until that review
  succeeds; it may supply test evidence but is not a clutch plugin ABI.
- [ ] Make a separate decision record for any ZIP archive format, manifest
  stability promise, signing, installation workflow, or external registry.

### Exit criteria

- The project explicitly records one of: remain local/source-only, begin a
  `.cslug` design, begin an ABI-v1 design, or stop the experiment.
- No experimental manifest field becomes a public compatibility promise by
  accident.

## Phase 7 — Reuse assessment and release decision

**Goal:** determine whether clutches simplify more than one capability before
stabilizing any interface.

### Todo

- [ ] Build a second, materially different clutch (the planned candidate is
  `slug.sqlite`) using the same resolver and scoped-registration boundaries.
- [ ] Build or simulate a third capability that does not primarily exercise
  filesystem or database resource lifetimes.
- [ ] Compare the three implementations for duplicated loader code, missing
  lifecycle hooks, diagnostic gaps, and manifest fields that vary per
  capability.
- [ ] Record whether a stable local manifest, packaged archive, installation
  workflow, or published plugin ABI is justified. If not, retain or remove the
  experimental feature rather than freezing an under-tested design.
- [ ] Before a release commitment, add conformance-style fixtures for all
  promised resolver and lifecycle behavior, update compatibility policy, and
  write the required decision records.

### Exit criteria

- At least three distinct providers demonstrate that module resolution,
  plugin ownership, and failure cleanup are reusable rather than tailored to
  `slug.io.fs`.
- A release proposal names the exact stable surface, migrations, compatibility
  version, security model, and validation suite—or explicitly concludes that
  clutches remain experimental.
