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

---

## Terminology

This experiment separates three concepts:

```text id="p4qax0"
module
    A Slug language/compilation unit.

clutch
    A composed and potentially distributable
    collection of modules and supporting artifacts.

plugin
    A native component that extends VM/runtime capability.
```

The corresponding artifacts are:

```text id="jmwv2s"
.slug       source representation of a Slug module
.cslug      compiled representation of a Slug module
.clutch     composed Slug library/application package
```

A `.slug` and corresponding `.cslug` represent the same conceptual module at different stages.

A module belongs to the language.

A clutch belongs to composition and distribution.

A plugin belongs to the runtime.

The central relationship is:

> **Programs import modules. Clutches distribute them. Plugins implement runtime capabilities they may require.**

---

# Modules

A module remains the unit visible to Slug source code.

For example:

```slug id="99vrzy"
val json = import("slug.json")
val fs = import("slug.io.fs")
```

An import asks for a module by identity.

It should not fundamentally mean:

```text id="u8s2kx"
load the file named slug/io/fs.slug
```

Instead:

> **`import("slug.io.fs")` means "give me the module named `slug.io.fs`."**

How that module is provided is a runtime concern.

---

## Module representations

A module may have a source representation:

```text id="ppl1bf"
fs.slug
```

and a compiled representation:

```text id="pzkjmh"
fs.cslug
```

These represent the same conceptual module:

```text id="u2f7vn"
              slug.io.fs
                  │
          ┌───────┴───────┐
          │               │
       fs.slug          fs.cslug
       source           compiled
```

The distinction matters to the compiler and runtime but should generally not matter to importing Slug code.

This gives us an important principle:

> **Module identity does not depend upon its physical representation.**

---

## Compiled-module compatibility

A distributed `.cslug` must identify the bytecode/runtime contract against which it was compiled.

The exact compatibility mechanism belongs to the `.cslug` design rather than the clutch design, but conceptually a
compiled module may identify something like:

```text id="n3o4mp"
module: slug.io.fs
format: cslug
bytecode: 3
```

When both representations are available, the runtime could prefer compatible compiled code:

```text id="m6rdc8"
compatible .cslug?
       │
      yes
       │
       ▼
load compiled module

       no
       │
       ▼
.slug available?
       │
      yes
       │
       ▼
compile and load
```

If only an incompatible `.cslug` exists, loading should fail explicitly.

For example:

```json id="n7a2fi"
{
  "error": "incompatible_compiled_module",
  "module": "slug.io.fs",
  "requiredBytecode": 3,
  "runtimeBytecode": 4
}
```

---

# Clutches

A clutch is the unit of **composition and distribution**.

A clutch provides one or more Slug modules and may carry everything required to make those modules work.

Conceptually:

```text id="v1ww8q"
Clutch
  │
  ├── Slug modules
  │     ├── .slug
  │     └── .cslug
  │
  ├── native runtime plugins
  ├── resources
  └── composition metadata
```

A clutch therefore distributes modules rather than source files.

For example, a source-oriented clutch could contain:

```text id="3e8h9p"
slug.uri.clutch
    modules/
        uri.slug
```

A release clutch could contain:

```text id="w6r3ko"
slug.uri.clutch
    modules/
        uri.cslug
```

A development clutch could contain both.

The clutch abstraction remains the same.

---

## One clutch, several modules

A clutch may naturally provide several related modules.

For example:

```text id="rdem2v"
sqlite.clutch
    modules/
        sqlite.cslug
        query.cslug
        migration.cslug
```

might advertise:

```text id="n6oq4j"
slug.sqlite
slug.sqlite.query
slug.sqlite.migration
```

Slug programs still import the individual modules:

```slug id="5gy7sz"
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

```text id="ykx6vh"
slug.io.fs.clutch
       │
       ├── slug.io.fs module
       │
       └── filesystem plugin
```

The three layers are:

```text id="v0kj6n"
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

```text id="by1td8"
slug.template.clutch
    parser.cslug
    template.cslug
    renderer.cslug
```

Native functionality is optional rather than intrinsic to clutches.

---

## The VM boundary

This experiment does **not** imply that Slug itself should become a collection of interchangeable plugins.

The VM continues to define fundamental Slug semantics.

Likely VM responsibilities include:

```text id="4mj4u1"
value representation
bytecode execution
functions and call frames
memory management
errors
module loading
native call boundary
runtime type/resource support
plugin/capability machinery
```

Plugins may extend what the VM can **do**, but should not redefine what Slug **means**.

Reasonable plugin-provided capabilities might include:

```text id="35iccs"
filesystem
networking
databases
cryptography
timers
channels
```

Plugins should not redefine:

```text id="37ncdl"
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

```text id="rzdtmr"
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

```slug id="0k8kxr"
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

```slug id="hax8jm"
export resource File

export foreign open(path:str):File
export foreign read(file:File):bytes
export foreign close(file:File):nil
```

A filesystem plugin supplies those implementations.

The consuming program only sees:

```slug id="t8wx82"
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

```text id="p4jdri"
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

```text id="e41n9j"
slug.sqlite          → sqlite.clutch
slug.io.fs           → io-fs.clutch
slug.arcade          → arcade.clutch
slug.arcade.audio    → arcade.clutch
```

Fundamentally, this is:

```text id="nd2q0v"
module identity → provider
```

Installation can establish this mapping once rather than requiring the runtime to scan every clutch whenever an import
occurs.

---

## Installation versus runtime resolution

Conceptually:

```text id="u6xmg1"
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

```text id="5oqts4"
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
        ├── select module representation
        ├── establish required capabilities
        ├── load plugin
        ├── validate foreign declarations
        └── expose module
```

The runtime does not need to discover which package might provide the requested module.

The repository already knows.

---

# Import semantics

Today import can approximately be thought of as:

```text id="9n8r9x"
import("foo")
      │
      ▼
resolve foo.slug
      │
      ▼
load module
```

The generalized model becomes:

```text id="vpbq0e"
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
                    ├── choose .cslug/.slug representation
                    └── expose module
```

This requires no dependency declaration in ordinary Slug source beyond the imports the program already contains.

No POM-shaped dependency description is required merely to tell the runtime what the source has already told it.

---

## Module resolution as an internal abstraction

Clutch support may eventually justify making module providers explicit internally.

Conceptually:

```rust id="rh4r9q"
trait ModuleResolver {
    fn resolve(&self, name: &ModuleName) -> Option<ModuleProvider>;
}
```

with possible providers such as:

```rust id="z47ac3"
enum ModuleProvider {
    Source(PathBuf),
    Compiled(PathBuf),
    Clutch(ClutchId),
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

For example:

```text id="4ih5zm"
slug.io.fs.clutch
│
├── clutch.toml
│
├── modules/
│   └── slug.io.fs.cslug
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

`clutch.toml` lives at the root of the archive and acts as the well-known entry point describing its contents.

The exact TOML schema is deliberately **not** defined by this experiment.

The implementation should first determine which metadata is genuinely required.

---

## TOML configuration

Slug-owned configuration and manifest files should use **TOML**, consistent with the existing Slug convention.

This includes clutch metadata:

```text id="on9agf"
clutch.toml
```

and repository metadata:

```text id="itkvn1"
manifest.toml
```

A future clutch manifest might conceptually contain information such as:

```toml id="gqoz09"
[clutch]
name = "slug.io.fs"
version = "0.1.0"

[modules]
"slug.io.fs" = "modules/slug.io.fs.cslug"

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

```text id="ijvtpd"
slug.io.fs.clutch/
├── clutch.toml
├── modules/
│   └── slug.io.fs.cslug
└── native/
    └── macos-aarch64/
        └── slug_io_fs.dylib
```

The important rule is:

> **An exploded clutch and a packaged clutch have the same logical contents and semantics; only their storage
representation differs.**

This allows the first experiment to concentrate on module resolution, plugin loading, resource types, FFI validation,
and lifecycle rather than ZIP implementation.

A future packaging step becomes conceptually:

```text id="pp88qd"
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

```text id="gaf6sg"
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

An executable clutch could contain:

```text id="o9exgw"
myapp.clutch
├── clutch.toml
├── app/
│   └── main.cslug
├── modules/
│   ├── slug.json.cslug
│   └── slug.sqlite.cslug
├── native/
│   └── ...
└── resources/
```

Its manifest could identify an entry module.

Conceptually:

```text id="bc4qtp"
type: executable
entry: app.main
```

Then:

```text id="bhct7w"
slug run myapp.clutch
```

could establish the clutch's embedded module repository, load its required capabilities, resolve the entry module, and
invoke `main`.

The important property is that imports inside the application remain ordinary Slug imports:

```slug id="kb26u5"
val json = import("slug.json")
val sqlite = import("slug.sqlite")
```

The executable clutch simply contains a closed set of providers capable of satisfying those imports.

---

## Closing the dependency graph

A future application build could start at its entry module, follow its imports and runtime requirements, and freeze the
resulting dependency closure into an executable clutch.

Conceptually:

```text id="rhqt56"
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

This gives the executable clutch an important property:

> **An executable clutch can represent a closed snapshot of the Slug modules and runtime capabilities required by a
program.**

That could make deployment extremely simple:

```text id="91kpkc"
application.clutch
+
compatible Slug runtime
=
runnable application
```

---

# Standalone and embedded builds

A closed executable clutch also creates a possible path toward standalone applications:

```text id="51qyrl"
Slug VM
+
embedded application.clutch
=
standalone executable
```

The `.clutch` remains the deployment/composition artifact.

The standalone executable merely embeds it with an appropriate runtime.

This distinction keeps packaging separate from runtime implementation.

---

## Constrained targets

The same mechanism could be especially useful for embedded targets such as ESP32.

For example, an embedded application might import only:

```slug id="k3t0fn"
val gpio = import("slug.gpio")
val time = import("slug.time")
```

Its closed dependency graph might therefore be:

```text id="6r6dau"
Slug VM core
+
application modules
+
slug.gpio
+
slug.time
+
ESP32 implementations
```

If nothing imports `slug.channel`, then the build need not include the channel module or its plugin.

If that also makes other capabilities unreachable, those may disappear as well.

The result is not a reduced version of the Slug language.

It is the same Slug VM running a smaller runtime world.

This suggests a future target build model:

```text id="gjmbz2"
application.clutch
        │
        +
target capability providers
        │
        ▼
closed target runtime
        │
        ▼
firmware / executable
```

This is a future direction rather than part of the initial clutch implementation, but it is an important architectural
consequence worth preserving.

---

# Machine-readable inspection

Clutches also create an opportunity for tooling and AI agents to inspect software composition without executing it.

For example:

```text id="kplv4q"
slug clutch describe sqlite --json
```

might report:

```json id="jsi1vq"
{
  "name": "sqlite",
  "modules": [
    "slug.sqlite"
  ],
  "representations": [
    "cslug"
  ],
  "native": true
}
```

while:

```text id="y28qvf"
slug module describe slug.sqlite --json
```

could describe the actual Slug interface.

This creates a useful distinction:

```text id="rw5n4a"
clutch.toml
    describes composition

module/type information
    describes the Slug API

repository manifest
    describes installed providers
```

Native-containing clutches should be clearly identifiable without executing their native code.

---

# Structured resolution errors

Because resolution is explicit, failures can also be explicit and machine-readable.

For example:

```json id="egc1f4"
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

```json id="7v4mr8"
{
  "error": "clutch_unavailable",
  "module": "slug.io.fs",
  "clutch": "slug.io.fs",
  "reason": "no native artifact for esp32"
}
```

This fits naturally with Slug's broader direction toward structured runtime diagnostics suitable for both humans and
agents.

---

# What this experiment deliberately does not solve

The clutch experiment does **not** currently propose:

- a public package registry;
- remote dependency resolution;
- a dependency version solver;
- lock files;
- signing infrastructure;
- automatic updates;
- a finalized `clutch.toml` schema;
- ZIP reading/writing;
- standalone executable generation;
- ESP32 tooling;
- arbitrary plugin hot unloading;
- the final concurrency/runtime boundary.

These are possible consequences of the architecture, not prerequisites for proving it.

The immediate experiment should remain small.

---

# First experiment: `slug.io.fs`

`slug.io.fs` is the preferred first clutch/plugin experiment.

It exercises the important architectural boundaries without introducing the unresolved concurrency question.

A deliberately small Slug API could be sufficient:

```slug id="prr6ux"
export resource File

export foreign open(path:str):File
export foreign read(file:File):bytes
export foreign close(file:File):nil
```

The experimental exploded clutch could be:

```text id="n0l9h6"
slug.io.fs.clutch/
├── clutch.toml
├── modules/
│   └── slug.io.fs.cslug
└── native/
    └── <current-platform>/
        └── filesystem plugin
```

The complete experimental path is:

```text id="xlhudv"
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
       ├── slug.io.fs.cslug
       └── filesystem plugin
                  │
                  ▼
              Slug VM
```

The experiment should answer:

1. Can `import("slug.io.fs")` treat a clutch-provided module exactly like an ordinary Slug module?
2. Can the repository map the module identity to its clutch provider?
3. Can the clutch provide `.cslug` without changing source-level import semantics?
4. Can the clutch establish a native plugin?
5. Can the plugin provide a typed `File` resource?
6. Can the runtime validate foreign declarations against the plugin implementation?
7. Can native errors cross the boundary cleanly?
8. Can plugin lifecycle and resource ownership be deterministic?
9. Does removing the clutch completely remove filesystem capability without changing Slug language semantics?
10. Does the design simplify the VM and FFI rather than merely moving their complexity elsewhere?

The key success criterion is:

> **After extracting `slug.io.fs`, removing its clutch should make filesystem capability cease to exist without
requiring changes to the Slug VM or language.**

The initial implementation should use an exploded `.clutch` directory.

ZIP packaging comes only after the architecture has been proven.

---

# Future validation

The filesystem experiment proves the mechanism once.

A useful later sequence would be:

```text id="e3mvpc"
slug.io.fs
    proves native runtime capability
    and typed resources

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

A module may have source (`.slug`) or compiled (`.cslug`) representations.

A clutch may distribute either representation together with native plugins and resources.

An installed clutch repository maps module identities to their providers.

An executable clutch may eventually close the complete dependency graph of an application into a shippable artifact.

A target build may eventually combine that closed graph with only the runtime capabilities required for the target.

The architecture can therefore be summarized as:

```text id="l7sffj"
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
                 slug.io.fs                 fs plugin
                .slug/.cslug                    │
                                                ▼
                                             VM core
```

And more succinctly:

> **The VM implements computation.**  
> **Plugins implement capability.**  
> **Modules expose capability.**  
> **Clutches distribute capability.**  
> **The repository resolves capability.**  
> **Executable clutches can close capability into a deployable world.**

For now, **clutch** remains an experimental architectural concept.

The next step is not more architecture.

The next step is to make `slug.io.fs` prove it.
