# General private frozen firmware programs

## Context

The ESP32-S3 proof executes the shared VM and has already proved a static
`slug.gpio`/`slug.time` provider path. Its desktop build step, however, accepts
only one hand-recognized blink source shape and renders one bespoke
`FlashProgram` instruction list. Adding another source feature currently
requires adding another AST recognizer and another firmware-only instruction
variant. That is not a viable way to compile an application program for the
board, and it risks creating an embedded language subset with semantics that
drift from the normal frontend.

The `.cslug` format remains a portable artifact format. The ESP32-S3 path does
not accept it from storage or transport, and this decision does not change
that boundary.

## Decision

Firmware builds will compile one user-selected Slug entry source and its
closed, capability-declared import graph through the ordinary frontend on the
desktop build host. The frontend will lower that graph once and render a
private Rust frozen-image input made of immutable data tables. Firmware will
validate and reconstruct the private in-process VM `Program` graph from those
tables at boot, then install only the static providers declared by its board
profile.

The selected source is a firmware-build input, not a runtime input. The build
interface will name it explicitly, have a documented default proof program,
and cause Cargo to rerun the build script when it changes. Its imports are
resolved only through the explicitly selected static providers and source
members; ordinary desktop roots, Clutches, device storage, and network loading
remain unavailable.

The frozen image is generated Rust for one exact firmware build. It is not
`.cslug`, a serialized bytecode format, a public Rust bytecode API, or a
promise that images are compatible across firmware revisions. The current
blink-specific AST recognizer and its `FlashInstruction` DSL will be removed
only after the general path loads and executes the blink program through the
same shared representation.

## Consequences

The device gets desktop-equivalent parsing, semantic checks, lowering, and
closed-graph validation without carrying a parser, resolver, or source files.
New source features need support in the generic frozen-image renderer and
loader rather than new per-example recognizers. Static provider manifests
remain the authority for firmware-native modules and logical board aliases.

The renderer and loader need explicit bounds and checked validation before a
program reaches the VM. Generated tables must not expose private VM layouts or
make host-provided values serializable. Firmware continues to rebuild and
reflash for every program change, and it continues to reject all unlinked
imports through normal checked module failures.

## Migration

The firmware initially keeps `programs/blink.slug` as its documented default.
The existing static GPIO proof becomes the first regression program for the
general frozen path. The old blink-only generator is deleted only once that
program has equivalent desktop and on-device evidence.
