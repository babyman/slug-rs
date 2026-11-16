# Builtins belong to the runtime

## Status

Accepted (2026-11-15).

## Supersedes

In part:

- [Implicit builtin module](2026-08-26-implicit-builtin-module.md): the
  bundled source file no longer defines values, and it is no longer found
  through module resolution, and programs can no longer import it.
- [Embedded builtin contract](2026-10-09-embedded-builtin-contract.md): an
  artifact no longer carries `slug.builtin`.

## Context

`slug.builtin` is meant to be the host's foundation module: the host
registers `print`, `println`, `len`, `keys`, `chan`, and `close`, and every
module sees them. Its source file, `lib/slug/builtin.slug`, is loaded like
any library module and does two jobs:

- it **declares** the host functions as `foreign`, so the compiler knows their
  types and which names exist
  ([unknown names are compile errors](2026-10-18-unknown-names-are-compile-errors.md)),
  and readers have one place to find them; and
- it **defines** `Error`, the standard error struct, in Slug.

The host functions exist whether or not the file does: when it is missing, the
loader serves a virtual `slug.builtin` built from the host's registrations.
`Error` exists only when the file is found and run. Every problem with the
module traces to that one definition:

- **Compiled artifacts lost `Error`.** `slug compile` bundled only explicit
  imports, so a `.cslug` had no `slug.builtin`, and a compiled program that
  threw `Error {...}` failed with ``unknown name `Error` ``, on the desktop and
  the ESP32-S3. Orble logged this as its finding 9. The fix (`f16bd1a`) bundles
  the module in every artifact, which costs every device program one of its
  four allowed modules.
- **A program can replace it.** The VM imports `slug.builtin` with no
  importer, which searches the module root before the library, and the
  compiler resolves it from each importer's directory first. A
  `slug/builtin.slug` beside a program replaces `Error` and changes which
  names the compiler accepts:

  ```text
  $ cat slug/builtin.slug
  export val Error = struct { type:str = "Hijacked", msg:str }
  export foreign println = fn(...values):nil
  $ slug p.slug          # p.slug throws Error {msg: "x"}
  slug: runtime error: uncaught throw: struct {"type": "Hijacked", "msg": "x"}
  ```

  and `print`, which the host still registers, becomes an unknown name.
- **What the program sees depends on where the library is.** Without the
  library, there is no `Error`, and the compiler cannot report unknown names.

## Decision

**The runtime defines every builtin. `slug.builtin` is their contract: a
declaration the compiler reads, never a module that runs.**

- **`Error` is a VM builtin**, installed like `cfg`, `int`, and `stacktrace`.
  The VM defines its struct, with the fields and defaults the contract
  declares: `type:str = "Error"`, `msg:str`, and `code`, `data`, and `cause`
  defaulting to `nil`. It needs nothing from the host, so every host and
  target has it, including `no_std` ones.
- **Host builtins stay host registrations**, as now: `print`, `println`,
  `len`, `keys`, and, where the host supports them, `chan` and `close`.
- **The contract comes from the host, not from module resolution.** The
  compiler takes the `slug.builtin` contract the host gives it. The CLI
  builds `lib/slug/builtin.slug` into its binary, so a `slug` executable
  always checks programs against the runtime it is. An embedded target's
  descriptor already names its contract,
  `clutch/slug.embedded.clutch/modules/builtin.slug`. No file found on the
  module path is ever read as `slug.builtin`, so a `slug/builtin.slug` in a
  program's directory, a project's `src/`, or the library has no effect.
- **The contract is never run or bundled.** It declares the host functions as
  `foreign` and `Error` as an exported struct, with doc comments. The
  compiler reads it for names, types, and documentation. The file runs nowhere,
  and no artifact carries it.
- **`slug.builtin` is not a module a program can import.** Its names are
  predeclared in every module, as Go's are; Go's `builtin` package likewise
  exists only to document them. `import("slug.builtin")` is a compile error
  that says so: "`slug.builtin` is not a module: its names, such as `Error`
  and `println`, are available in every module without an import". A module
  that declares a builtin's name replaces it there, as now, and one that
  still needs the builtin renames its own declaration.
- **Tests keep the contract and the runtime equal.** For each host and
  target, every `foreign` declaration has a registered function with a
  matching arity, every registered builtin is declared, and the contract's
  `Error` has the VM's fields and defaults. A builtin that is added in one
  place and not the other fails the build, not a program.

## Consequences

`Error` and the host functions exist in every program on every host, compiled
or not, and nothing a program ships can change them. Compiling, running, and
the embedded build all agree, because they all take the contract from the host.

With nothing to import, the runtime needs no `slug.builtin` module at all:
the virtual module the loader builds from host registrations, and the
artifact host's lookup for it, go away. The only explicit imports in this
repository outside tests are in two agent eval workspaces, both
`val {Error} = import("slug.builtin")`: agents that did not know `Error` is
always there. The error teaches that directly.

Artifacts stop carrying `slug.builtin`. The ESP32-S3 blink, hello, PinPulse,
and tasks programs go back to three modules of their four, and no descriptor
limit changes. `slug report` no longer needs to hide the carried module.

`Error`'s definition moves from Slug into the VM, so it is written twice, in
the VM and in the contract, kept equal by the parity tests. Adding a
Slug-defined foundation value now means VM work. That friction is
intentional: the module is meant to stay small, and the
[implicit builtin module](2026-08-26-implicit-builtin-module.md) decision
already sends ordinary helpers to explicit modules.

The compiler's separate list of VM functions (`cfg`, `int`, `stacktrace`),
which [unknown names are compile errors](2026-10-18-unknown-names-are-compile-errors.md)
records as a cost, can become declarations in the contract. That is left
for a separate change. Those functions are the VM's, not the host's, so
declaring them needs a form other than `foreign`.

## Migration

A program that imports `slug.builtin` drops the import and uses the names
directly. A program that defined its own `slug/builtin.slug` to replace
builtins loses that; none exists in this repository or in Orble. Tests that
import `slug.builtin` explicitly are rewritten.

An artifact compiled with `f16bd1a` carries a `slug.builtin` module that the
runtime no longer uses; it still runs. Artifacts have no compatibility
promise, so nothing reads that module or rejects it.

Documents to update when implemented: the specification's "Implicit builtin
module" section and its note on unknown names, `language/runtime-requirements.md`,
`language/embedded-host-modules.md`, the header comment of
`lib/slug/builtin.slug` and of the embedded contract, and the superseded
decisions' status lines.
