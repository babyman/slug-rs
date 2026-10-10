# PinPulse light-walk output profile

## Context

The first numeric ESP32-S3 profile conservatively excluded boot, strapping,
and RGB-connected pins. The PinPulse Shield has a direct LED on each of those
broken-out pins, and its board-specific light-walk has been physically tested
with all of them driven as push-pull outputs.

## Decision

The PinPulse firmware profile permits this tested ordered set: GPIO 4--7,
15--18, 8, 3, 46, 9--14, 1, 2, 42--38, 0, 45, 48, 47, and 21. It continues to
exclude UART GPIO 43/44, native USB GPIO 19/20, and PSRAM GPIO 35--37.

The default Slug program uses that set to initialize every LED low, chase one
LED at a time, display a full-lap flash, and clear the lap. It uses ordinary
numeric GPIO writes and tail recursion; no firmware-specific source construct
is added.

This supersedes the ESP32-S3 board-list portion of
[Embedded GPIO numbers](2026-10-09-embedded-gpio-numbers.md).

## Consequences

The profile accurately represents the tested PinPulse hardware rather than a
generic conservative ESP32-S3 policy. Other boards must declare and validate
their own output sets. The module remains output-only and still does not grant
mode selection, inputs, interrupts, PWM, ADC, or arbitrary electrical
configuration.

## Migration

None for numeric GPIO callers. The profile makes additional physical numbers
available on this specific board.
