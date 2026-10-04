# `.cslug` whole-program artifact plan

## Goal

Implement `.cslug` as a compact, binary, validated persistence and transport
format for a complete VM `CompiledProgram`, without changing the direct source
execution path or publishing a VM-independent IR.

## Preconditions

- [ ] Keep the artifact scope to one whole program with a defined entry point;
  defer module artifacts and Clutch loading.
- [ ] Inventory every `CompiledProgram` component that affects execution,
  module initialization, source diagnostics, or validation.
- [ ] Specify the version-1 header: magic bytes, format version, VM/bytecode
  compatibility version, flags, byte order, and canonical integer encoding.
- [ ] Specify bounded section framing, all canonical encodings, resource
  limits, verifier rules, and checked load-error categories in
  `reference/compiled-artifacts.md`.

## Implementation slices

### 1. Establish the codec boundary

- [ ] Add a narrowly owned `cslug` codec surface with `encode` and `decode`;
  avoid making the artifact layout a public Rust bytecode API.
- [ ] Define an owned checked load-error type that is converted at the CLI and
  host boundaries without exposing host panics.
- [ ] Keep direct `.slug -> CompiledProgram -> VM` execution unchanged and
  add a regression proving it does not invoke the codec.

### 2. Encode the complete executable state

- [ ] Encode the program header and entry point first, then constants,
  functions, bytecode, module state, and diagnostic/source metadata.
- [ ] Require canonical ordering and reject values that cannot be represented
  by version 1 rather than silently widening the schema.
- [ ] Add deterministic byte-for-byte encoding tests for representative
  programs and module graphs.

### 3. Decode and verify before installation

- [ ] Parse only bounded, length-delimited sections with explicit limits for
  file size, allocations, nesting, tables, constants, and instructions.
- [ ] Validate every reference, jump target, function/call descriptor, module
  reference, entry point, and executable invariant before constructing the
  returned `CompiledProgram`.
- [ ] Fuzz and regression-test truncated, oversized, invalid-version, invalid
  flag, malformed-index, malformed-constant, and invalid-control-flow inputs.
- [ ] Prove every failure is a checked load error and no partial program can
  execute.

### 4. Add host-facing persistence only after codec proof

- [ ] Add explicit CLI or embedding operations to write and load whole-program
  artifacts, with clean format/VM-compatibility diagnostics.
- [ ] Test a program encoded on one supported machine/build and loaded on
  another compatible machine/build.
- [ ] Test incompatible versions and verify that recompilation is the stated
  recovery path.

### 5. Evaluate ESP deployment separately

- [ ] Compare `.cslug` size, decode time, RAM use, and boot time against the
  private frozen firmware image on the selected ESP path.
- [ ] Decide separately whether firmware accepts `.cslug` from a trusted build
  input, retaining its existing untrusted-input validation and resource limits.
- [ ] Do not replace the frozen-image boundary or add device storage, Clutch
  module artifacts, memory mapping, or zero-copy execution without a new
  decision.

## Completion gate

Version 1 is complete only when the format specification, codec, verifier,
fixtures, public diagnostics, focused VM/CLI coverage, cross-compatible-build
test, malformed-input suite, and `make check` all pass. The private frozen
firmware image must remain explicitly distinct from `.cslug`.
