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

## `slug.gpio`

When selected, `slug.gpio` exports:

```slug
foreign write = fn(name:str, level:bool):nil
```

`name` is a host-defined logical output alias, not a raw pin number. A host
maps aliases such as `"led"` to its safe output configuration. Unknown aliases
and unavailable output capability are checked host failures. `level` is true
for the host-defined active level and false for its inactive level.

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
