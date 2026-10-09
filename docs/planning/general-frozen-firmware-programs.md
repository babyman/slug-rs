# General private frozen firmware program plan

## Goal

Replace the ESP32-S3 proof's blink-specific source recognizer with a general,
closed-program frozen-image pipeline. A firmware build selects one Slug entry
source, compiles it on the desktop host through the ordinary frontend, embeds
validated private image tables in flash, and starts it through the shared VM.

This plan retains the private frozen-image boundary. `.cslug` remains a
separate portable artifact and is not accepted by firmware.

## Delivery slices

### 1. Establish the private generic image boundary

- [ ] Define a `slug-vm`-owned, `no_std + alloc`-compatible frozen-image
  table contract and checked loader errors.
- [ ] Keep the contract private to generated firmware Rust input; do not add a
  public bytecode serialization API or accept byte streams on the device.
- [ ] Prove the loader rejects malformed table references before installation.

### 2. Render ordinary lowered programs

- [ ] Add a frontend-owned renderer from a closed compiled graph to the
  frozen-image table contract.
- [ ] Preserve constants, chunks, call and source metadata, entrypoint
  selection, and graph member identities required by the selected subset.
- [ ] Reject runtime-only values and unsupported provider bindings with checked
  desktop build diagnostics.

### 3. Select a firmware entry program

- [ ] Make the firmware build script select a documented Slug entry path,
  defaulting to `programs/blink.slug`, and register source dependencies with
  Cargo rerun directives.
- [ ] Compile against an explicit static-provider manifest, with no desktop
  filesystem roots, dynamic Clutches, or device resolver fallback.
- [ ] Exercise an alternate program path in a host-side build-script test.

### 4. Replace the blink-specific loader

- [ ] Load a frozen graph and invoke its validated root entrypoint in firmware.
- [ ] Run the existing blink source without `frozen_gpio` or
  `FlashInstruction`-specific firmware code.
- [ ] Retain checked boot diagnostics and bounded heap behavior.

### 5. Verify on board and document the workflow

- [ ] Cross-build the selected firmware and flash the existing ESP32-S3 board.
- [ ] Capture UART output for the default blink and one alternate selected
  program, including image size and heap/stack limits.
- [ ] Remove the deprecated specialized generator and update the firmware
  instructions, changelog, and proof record.

## Completion gate

The pipeline is complete when a clean firmware build can select a source
program, compile its declared closed graph with normal frontend semantics,
validate and run it from flash on the shared VM, and reproduce that execution
on the ESP32-S3 without source parsing, runtime loading, or a portable artifact
format.
