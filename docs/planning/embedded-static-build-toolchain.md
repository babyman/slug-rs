# Embedded static build toolchain

## Goal

Make this deployment path reusable across applications and board packages:

```text
.slug source -> closed `.cslug` -> firmware ROM image with static providers -> flash
```

The device executes only an artifact embedded in the firmware image. It does
not parse source, inspect target descriptors, load a `.cslug` file from device
storage, discover a provider, or dynamically link a library.

This plan implements [Build embedded firmware from closed `.cslug`
artifacts](../decisions/2026-10-09-embedded-static-build-toolchain.md).

## Definition of done

- A board-independent build component accepts an explicit entry source and
  versioned target descriptor and emits a closed `.cslug` build input.
- A descriptor declares the board runtime package, source contracts, static
  provider libraries, and decoder/resource limits.
- Build-time validation proves imports are closed and every declared foreign
  function has exactly one selected static provider.
- An ESP32-S3 package embeds the generated artifact, links the selected
  provider libraries, and can flash a firmware image without source frontend
  code on the device.
- A second application can be selected without editing firmware Rust source.

## Work packages

### 1. Specify and parse the target descriptor

- [ ] Add a versioned descriptor format owned by a target-neutral build crate.
- [ ] Require an entry-source path, board/runtime package identifier, artifact
  limits, and an ordered list of source-contract/provider pairs.
- [ ] Reject duplicate module names, paths outside the descriptor root,
  unsupported descriptor versions, zero/unbounded limits, and providers that
  are not linked by the selected board package.
- [ ] Document whether the first descriptor is TOML and reserve a stable
  diagnostic namespace for descriptor failures.

Acceptance: parser and validation tests cover valid, malformed, duplicate,
out-of-root, and incompatible descriptors without requiring an ESP toolchain.

### 2. Extract target-neutral artifact compilation

- [ ] Move the host-side compilation currently in
  `firmware/slug-esp32-s3/build.rs` into a reusable build crate or CLI.
- [ ] Build `StaticModuleResolver` exclusively from descriptor contracts.
- [ ] Compile the entry graph through `ModuleHost::compile_artifact`, encode
  `.cslug`, then decode it with the target limits before emitting it.
- [ ] Emit Cargo rebuild directives for the entry source, every selected
  contract, and the descriptor; do not rely on a hard-coded blink filename.

Acceptance: two fixtures produce distinct closed artifacts; a dynamic or
unselected import fails at build time with its source location.

### 3. Make static provider libraries declarative

- [ ] Define a small target-native provider registration interface that reports
  its module/function identities and registers callbacks into `ArtifactHost`.
- [ ] Validate descriptor contracts against provider declarations in both
  directions: no missing declared foreign function and no extra registration.
- [ ] Keep native callbacks, resources, and HAL state out of `.cslug`.
- [ ] Convert ESP32-S3 builtins, GPIO, and time from firmware-local special
  cases to the registration interface.

Acceptance: desktop tests cover matching providers, missing functions, extra
functions, duplicate providers, and a provider callback returning a checked
runtime failure.

### 4. Parameterize the ESP32-S3 package

- [ ] Add the PinPulse descriptor with `blink.slug` as its default entry.
- [ ] Replace `compile_blink_artifact`, `slug_blink.cslug`, and
  `load_blink_artifact` with descriptor-derived names and generated metadata.
- [ ] Keep the board package responsible for heap/stack/linker policy, GPIO
  pin ownership, serial output, and `espflash` documentation only.
- [ ] Make release builds record artifact size, firmware size, and configured
  heap/stack limits in machine-readable build output.

Acceptance: `cargo +esp build --release` succeeds for the default descriptor,
and a second selected program changes only build inputs—not firmware Rust
source.

### 5. Establish validation and release gates

- [ ] Add target-neutral unit/integration coverage to the normal workspace
  test suite.
- [ ] Add an ESP cross-build job that runs without attached hardware.
- [ ] Add a board-owned manual flash/UART acceptance record with the exact
  descriptor and artifact hash.
- [ ] Measure artifact decode time and peak heap use before raising default
  artifact limits or adding broader embedded capabilities.

Acceptance: a pull request cannot add an embedded contract, provider, or
artifact-limit increase without the corresponding host tests and cross-build.

## Explicit non-goals

This work does not add a source-language dialect, runtime `.cslug` loading,
device filesystems, OTA delivery, dynamic libraries, C ABI stabilization,
automatic capability discovery, or embedded concurrency. Those require
separate authority, lifecycle, and memory decisions.
