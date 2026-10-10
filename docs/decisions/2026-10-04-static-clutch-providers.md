# Static Clutch providers

## Context

The desktop Clutch system activates native modules through desktop loader
configuration and dynamically loaded libraries. That delivery mechanism is not
available to bare-metal firmware, but embedded programs still need to import
host capabilities through the same source-level module identities as desktop
programs.

The first useful embedded capability is a status LED. It needs a GPIO output
and a short delay, while preserving the shared Slug VM and avoiding raw board
pin access in application source. The existing frozen-firmware-image boundary
also requires source compilation and capability selection to happen on the
desktop build host.

## Decision

Slug will use **static Clutch providers** for firmware-native modules. A static
provider has the same source-visible module identity and callable signature as
a desktop Clutch, but is selected and linked at firmware build time rather than
being discovered or loaded dynamically on the device.

The first static-provider contract is:

```slug
val gpio = import("slug.gpio")
gpio.write("led", true)
```

`slug.gpio.write(name, level)` accepts a board-defined logical output name and
a boolean level. Firmware configuration maps `"led"` to the selected safe
GPIO output. Slug source must not name raw pin numbers or configure pin modes.
Unknown names, unsupported capabilities, and invalid native arguments remain
checked Slug failures.

The initial blink path will also provide `slug.time.sleepMs(milliseconds)` as
an explicitly blocking host call. It is not an implementation of the language
timer or concurrency facilities; asynchronous timers and host-produced events
remain a separate design problem.

The desktop and firmware implementations may differ in their physical effects:
a desktop provider may record simulated writes while the ESP provider owns an
`esp-hal` output pin. They must retain the same module names, exported
signatures, argument checks, and observable Slug-level error behavior for the
capabilities they both expose.

Static providers are registered explicitly by the embedded host alongside the
frozen entry program. They are neither device-resolved modules nor a new source
syntax, and they do not alter the existing desktop Clutch activation path.

## Consequences

Firmware gets a small, auditable capability surface with no filesystem,
dynamic-library, or raw-board API dependency. A board profile becomes the sole
place that selects the LED pin and its electrical configuration. The same Slug
example can be exercised against a desktop simulation before hardware flash.

The frozen-image generator must expand beyond the current UART hello shape to
compile imports, calls, booleans, and the selected entry functions. Firmware
build metadata must declare both the selected static providers and their board
aliases, so an image cannot silently request a capability that the host did not
link.

This decision intentionally excludes arbitrary GPIO access, input interrupts,
PWM, ADC, Wi-Fi, BLE, dynamic provider discovery, and a portable compiled
artifact. Each needs a narrower contract and separate acceptance tests.

## Migration

None. Existing desktop Clutches and the ESP32-S3 UART proof remain unchanged.
This decision complements [Host-owned module resolution]
(2026-09-23-host-owned-module-resolution.md) and applies the
[Frozen firmware image boundary](2026-10-04-frozen-firmware-image-boundary.md)
to native firmware capabilities.
