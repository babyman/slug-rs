# Embedded GPIO numbers

## Context

The original static-provider proof exposed board-defined GPIO aliases such as
"led". That made an ordinary GPIO number unavailable to a program while still
requiring the project to know which physical output its wiring used. Project
names such as motor_enable are application concerns, not board capabilities.

## Decision

slug.gpio.write accepts a physical GPIO number and a boolean level. Each
embedded host declares a finite allow-list of safe output numbers and rejects
all other numbers as checked host failures. A valid number is finite,
non-negative, and whole.

The ESP32-S3 N16R8 profile allows GPIO 1, 2, 4--18, 21, 38--42, and 47.
It lazily configures a selected pin as a push-pull output on its first write.

This supersedes the GPIO naming rule in
[Static Clutch providers](2026-10-04-static-clutch-providers.md).

## Consequences

Programs express their direct physical wiring without firmware-specific
aliases, while the firmware still prevents use of USB, UART, PSRAM, strapping,
and RGB pins. Programs need their own names when names improve readability.
The capability remains output-only: it does not grant mode selection, input,
interrupt, PWM, ADC, or arbitrary electrical configuration.

## Migration

Replace gpio.write("led", level) with gpio.write(2, level), or the physical
number selected by the project wiring. Existing alias-based embedded source is
not source-compatible.
