# `.cslug` whole-program artifact plan

## Goal

Implement `.cslug` as a compact, binary, validated persistence and transport
format for a complete VM `CompiledProgram`, without changing the direct source
execution path or publishing a VM-independent IR.

## Preconditions

- [x] Keep the artifact scope to one closed compiled module graph with a
  defined root entry point; defer module artifacts and Clutch loading.
- [x] Inventory every `CompiledProgram` component that affects execution,
  module initialization, source diagnostics, or validation.
- [x] Specify the version-1 header: magic bytes, format version, VM/bytecode
  compatibility version, flags, byte order, and canonical integer encoding.
- [x] Specify bounded section framing, all canonical encodings, resource
  limits, verifier rules, and checked load-error categories in
  `reference/compiled-artifacts.md`.

### Current executable-state inventory

The current compiler's private `CompiledProgram` is one `slug_vm::Program`
plus frontend-only global-binding facts. The serializable executable state is
therefore the closed graph of `Program` values compiled by `ModuleGraph`, not
that private lowering wrapper or any live `ModuleInstance` values.

Each graph member must retain its logical module key and runtime module name,
and the graph must identify its root member. A member's `Program` retains:

- chunks, their constants, parameters, local counts, callable identity, packed
  instructions, and source-span references;
- bindings, declarations, exports, entrypoint metadata, and callable
  identities;
- interned source paths/spans and every executable metadata pool: global names,
  captures, schema and struct fields, match patterns, interpolations, list
  spreads, call descriptors, selected calls, and select cases.

The codec must not capture `ModuleGraph` caches, semantic snapshots, initialized
module values, VM frames, task/scheduler state, native registrations, host
configuration, or activation leases. Those remain fresh host/VM state. The VM
installation validator remains the final executable-invariant check after
decode; the decoder must additionally validate file structure and graph
references before a `Program` reaches installation.

Version 1 uses the fixed `CSLG` header, format and VM/bytecode compatibility
version `1`, zero flags, little-endian fixed-width fields, and canonical
unsigned LEB128 lengths, counts, and indexes. The decoded graph is the sole
import provider: an absent dynamic or static import fails through the ordinary
checked module-load path rather than falling back to source resolution.

Version 1 artifacts are produced through the Slug frontend's closed-graph API.
Because the public Rust bytecode API does not retain construction provenance,
the encoder validates representability and rejects non-source runtime values,
including native functions/resources, channels, tasks, closures, bindings, and
overload groups.

## Implementation slices

### 1. Establish the codec boundary

- [x] Add a narrowly owned `cslug` codec surface with `encode` and `decode`;
  avoid making the artifact layout a public Rust bytecode API.
- [x] Define an owned checked load-error type that is converted at the CLI and
  host boundaries without exposing host panics.
- [x] Keep direct `.slug -> CompiledProgram -> VM` execution unchanged and
  add a regression proving it does not invoke the codec.

### 2. Encode the complete executable state

- [x] Encode the program header and entry point first, then constants,
  functions, bytecode, module state, and diagnostic/source metadata.
- [x] Require canonical ordering and reject values that cannot be represented
  by version 1 rather than silently widening the schema.
- [x] Add deterministic byte-for-byte encoding tests for representative
  programs and module graphs.

### 3. Decode and verify before installation

- [x] Parse only bounded, length-delimited sections with explicit limits for
  file size, allocations, nesting, tables, constants, and instructions.
- [x] Validate every reference, jump target, function/call descriptor, module
  reference, entry point, and executable invariant before constructing the
  returned `CompiledProgram`.
- [x] Fuzz and regression-test truncated, oversized, invalid-version, invalid
  flag, malformed-index, malformed-constant, and invalid-control-flow inputs.
- [x] Prove every failure is a checked load error and no partial program can
  execute.

### 4. Add host-facing persistence only after codec proof

- [x] Add explicit CLI or embedding operations to write and load whole-program
  artifacts, with clean format/VM-compatibility diagnostics.
- [x] Test a program encoded on one supported machine/build and loaded on
  another compatible machine/build.
- [x] Test incompatible versions and verify that recompilation is the stated
  recovery path.

### 5. Evaluate ESP deployment separately

- [x] Compare `.cslug` size, decode time, RAM use, and boot time against the
  private frozen firmware image on the selected ESP path.
- [x] Decide separately whether firmware accepts `.cslug` from a trusted build
  input, retaining its existing untrusted-input validation and resource limits.
- [x] Do not replace the frozen-image boundary or add device storage, Clutch
  module artifacts, memory mapping, or zero-copy execution without a new
  decision.

### 6. Prepare the closed artifact runtime for firmware

- [x] Move the closed-artifact runtime host into `slug-vm::cslug` so a
  firmware image can execute a decoded graph without linking `slug-frontend`.
- [x] Preserve checked closed-graph imports and live exports, and expose a
  host-facing registration API for statically linked module-qualified native
  functions.
- [x] Have the firmware build compile the selected source graph and its static
  providers into a version-1 `.cslug` file in `OUT_DIR`.
- [x] Embed and decode that file under finite ESP limits, register the selected
  static providers, and replace the temporary frozen-table loader.

## Completion gate

Version 1 is complete only when the format specification, codec, verifier,
fixtures, public diagnostics, focused VM/CLI coverage, cross-compatible-build
test, malformed-input suite, and `make check` all pass. The private frozen
firmware image must remain explicitly distinct from `.cslug`.

The ESP evaluation and decision are recorded in
[`2026-10-04-cslug-esp-deployment.md`](../decisions/2026-10-04-cslug-esp-deployment.md).
