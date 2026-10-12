# Embedded host modules

An embedded or otherwise restricted Slug host MAY make explicitly selected
native capabilities available through source modules. These are ordinary
`import(...)` modules: their names, exports, callable signatures, and checked
errors are part of the source contract, while their physical effects remain
host-specific.

## Availability

A host MUST resolve an optional host module only when it has selected both the
module source and every declared foreign implementation required to initialize
it. An unselected module MUST fail through the ordinary checked module-import
error. A host MUST NOT silently provide a substitute capability.

## Embedded build selection

An embedded build selects its optional modules before compilation, outside the
Slug source program. The selected module contracts and their native
implementations form the complete capability set for that firmware image. A
program that imports any other module MUST receive the ordinary checked
module-import error during the host-side build; the device MUST NOT attempt
source resolution, provider discovery, or a fallback capability at runtime.

Every selected `foreign` declaration MUST have exactly one statically linked
native implementation for the target. Conversely, a target MUST NOT register a
foreign function that is absent from its selected source contracts. Native
implementations are target-owned code and state; `.cslug` contains the closed
source-derived graph and foreign identities, not native code or hardware
handles.

These rules add no source syntax. An embedded target chooses its entry source,
contracts, provider libraries, and resource limits through its build
descriptor.

## `slug.builtin`

The initial embedded profile provides the ordinary implicit and explicitly
importable `slug.builtin` module with `Error`, `print`, `println`, `len`, and
`keys`. `print` and `println` write to the host diagnostic stream. `len` and
`keys` have the same behavior as on the desktop host.

The profile deliberately omits `chan` and `close` while embedded concurrency is
unavailable. VM-provided `cfg`, `int`, and `stacktrace` remain available; the
current firmware supplies an empty configuration and empty entry arguments.

## `slug.gpio`

When selected, `slug.gpio` exports:

```slug
foreign write = fn(pin:num, level:bool):nil
```

`pin` is a finite, non-negative whole physical GPIO number. The host selects a
finite allow-list of safe output pins; a pin outside that list, a fractional
number, or an unavailable output capability is a checked host failure.
Project-level names such as `motor_enable` belong in source code, not in this
module. `level` is true for electrical high and false for electrical low.

This module does not grant input access, pin-mode selection, interrupt
registration, PWM, ADC, or arbitrary electrical configuration.

## `slug.time`

When selected for the initial embedded profile, `slug.time` exports:

```slug
foreign sleepMs = fn(milliseconds:num):nil
```

`sleepMs` blocks the current host execution for a finite, non-negative integral
number of milliseconds within the host's documented bound. Negative,
fractional, non-finite, or out-of-range values are checked host failures.
`sleepMs` does not create a timer, task, or background host producer; it is not
an alternative spelling of `after`.
