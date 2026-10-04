# ESP32-S3 Slug proof of concept

## Goal

Flash a bare-metal firmware image to an ESP32-S3 that executes a Slug program
embedded in flash and writes the expected line to UART:

```slug
val main = fn() {
    println("Hello from Slug on ESP32-S3")
}
```

This plan implements the boundary adopted by
[Embedded host capabilities](../decisions/2026-10-03-embedded-host-capabilities.md).
It is deliberately a functional and measurable baseline rather than an
embedded distribution format or full device platform.

## Scope

The firmware includes startup, an explicit allocator, UART, an embedded host,
and the VM. A desktop build tool will compile the entry source and emit a
private frozen firmware image that the device translates to checked in-memory
bytecode at boot.

The proof of concept excludes:

- filesystem, network, environment-variable, and command-line configuration;
- external import roots, Clutches, dynamic native libraries, and device
  storage;
- Slug concurrency, timers, and background host producers;
- a source package format, bytecode serialization, or `.cslug` implementation;
- general GPIO, Wi-Fi, or BLE APIs.

## Portability invariants

The firmware is a port of one Slug runtime, not an embedded language variant.
The VM, bytecode validation, source semantics, and checked failures must match
the desktop implementation for the subset selected by the host. A host may
change only its explicit capabilities and report an unavailable platform
capability through a checked failure.

Keep Cargo selection narrow: `std` versus `no_std + alloc`, and the selected
concurrency implementation, are build constraints. Do not add per-device
language or builtin feature flags. Device functions are registered by the host
at runtime, and ordinary pure Slug modules are selected by its resolver.

Every firmware boot creates a new host and VM. If a future soft reset is added,
it must reconstruct this state or prove it releases every VM-owned root before
executing another program.

## Design boundaries

```text
firmware bootstrap
  -> allocator and UART initialization
  -> embedded host
       -> native capability registry: println -> UART
       -> flash program image: entry module only
  -> VM: invoke main()
```

The flash module resolver maps an explicitly known logical name to source. It
does not register builtins or hardware functions. The embedded host owns native
capability registration, so a source module cannot gain device access merely by
being resolvable.

## Implementation phases

### 1. Prove the ESP32-S3 toolchain and board path

Install and use the Espressif Xtensa Rust toolchain, generate a minimal
`esp-hal` S3 firmware shell, and flash it over the board's USB serial path.
Verify a Rust-only UART line before linking Slug code.

Acceptance: a documented command flashes the shell and its serial monitor
shows a known boot line after reset.

### 2. Add the firmware package without changing desktop behavior

Add a narrowly scoped ESP32-S3 firmware package and its target configuration.
It must not become a default workspace build target. Declare the selected chip,
serial settings, panic behavior, linker configuration, and an initial bounded
heap in repository-owned configuration or documentation.

Acceptance: the package cross-builds in optimized embedded mode, while the
existing desktop format, lint, and test commands remain unchanged.

### 3. Establish `no_std + alloc` in the runtime path

Make the VM and only the frontend path required for the hello program compile
without `std`. Move heap-backed portable types to `alloc`; isolate desktop-only
services behind a `std` feature or keep them in desktop crates. Select an
embedded-compatible replacement for every runtime-required `std` collection.

Do not introduce a garbage collector. Preserve reference-counted value
ownership and provide the allocator explicitly from firmware startup.

Acceptance: a host-side no-`std` check compiles the selected VM/frontend
feature set, and an embedded-core regression target proves that it needs no
filesystem, environment, desktop loader, or standard streams. A direct VM
bytecode smoke test runs on the board without allocation failure.

### 4. Implement the embedded host and resolver

Create an embedded host assembly that supplies fixed configuration, a
compiled-in entry program image, and deny-by-default import behavior. Reuse
the desktop frontend at build time rather than duplicating source compilation
on the device. Make unsupported imports return the normal checked module
error.

Acceptance: host tests prove entry resolution and checked rejection of an
unlisted import; firmware loads the generated entry image without filesystem
access.

### 5. Register UART output as the first native capability

Register `println` in the embedded host through the existing checked native
call boundary. Its implementation converts only supported values and writes
the result to UART. Keep source resolution independent of this registration.

Acceptance: a firmware program calling `println` emits one complete expected
line; malformed native arguments remain checked runtime failures rather than
host panics.

### 6. Flash, measure, and document the end-to-end result

Flash the firmware with `espflash` or the chosen supported tool, capture the
serial transcript after a reset, and publish reproducible build/flash/monitor
instructions. Record firmware flash size, static RAM, configured heap, peak
heap use for the hello program, and boot-to-output time.

Acceptance: a clean checkout with the documented toolchain can cross-build,
flash, and observe `Hello from Slug on ESP32-S3` on the selected board. The
firmware reports or resets deliberately on heap exhaustion; it never relies on
undefined behavior or a host panic.

## Validation ladder

During implementation, run the narrowest relevant Rust tests first. Before a
handoff, run `make check` for the desktop workspace plus the embedded
cross-build and on-device UART acceptance test. Add host-side regression tests
for resolver denial, native-registration ownership, and source/runtime errors.
Keep the embedded-core regression target in ordinary CI; reserve board flashing
for the documented hardware acceptance step.

## Follow-up decisions

After the proof of concept, decide from measurements whether to add more native
modules, a better embedded map implementation, source compression, or portable
`.cslug` artifacts. The private frozen-image boundary is defined by
[Frozen firmware image boundary](../decisions/2026-10-04-frozen-firmware-image-boundary.md);
portable artifacts remain a separate decision.

The next selected capability is deliberately narrower: the
[ESP32-S3 static Clutch GPIO plan](esp32-s3-static-clutch-gpio.md) defines a
logical LED output and blocking delay through static providers, without using
the desktop dynamic Clutch loader.
