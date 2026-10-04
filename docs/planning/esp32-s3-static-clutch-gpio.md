# ESP32-S3 static Clutch GPIO plan

## Goal

Reach a small, observable Slug program that turns a selected ESP32-S3 LED on
and off repeatedly, without creating an embedded-only language or depending on
the desktop dynamic Clutch loader.

The target source shape is:

```slug
val gpio = import("slug.gpio")
val time = import("slug.time")

val blink = fn() {
    gpio.write("led", true)
    time.sleepMs(250)
    gpio.write("led", false)
    time.sleepMs(250)
    recur()
}

val main = fn() {
    blink()
}
```

The first hardware milestone may run one on/off cycle rather than recur. The
full acceptance target is repeated transitions; `recur()` must remain a
checked, bounded-by-stack-language-semantics tail call, not a device-specific
loop construct.

## Boundaries

- `slug.gpio` and `slug.time` are normal imported modules with declared
  signatures. Firmware implements them as static Clutch providers; desktop can
  provide a simulator with the same public contract.
- Firmware configuration owns `"led" -> GPIO pin`, output polarity, and
  electrical setup. Application source owns neither a pin number nor a mode.
- The frozen-image build step resolves imports and verifies the selected
  provider/capability manifest on the desktop. The device loads only the
  linked, checked image and explicitly registered providers.
- `sleepMs` is synchronous for this first path. It does not enable the
  scheduler, `after`, or background interrupt producers.
- The image remains private firmware build input, not a `.cslug` artifact.

## Delivery stages

### 0. Identify the board LED safely

Record the exact ESP32-S3 board model and either its onboard LED GPIO/polarity
or the GPIO used for an external LED with a resistor. Confirm the pin is not
reserved for USB, flash/PSRAM, boot strapping, or another active board
function.

Acceptance: a board profile names one logical alias, `led`, and one verified
safe output configuration. No generic GPIO number is guessed in Slug source.

### 1. Define static-provider registration

Add a small host-facing registration contract for statically linked native
modules. It must associate a module identity, exported callable signatures,
and checked callbacks without referencing filesystem paths, shared libraries,
or desktop configuration.

Keep the current desktop dynamic Clutch loader intact. If a common type is
useful, place it with the loader contracts rather than making the VM know about
Clutches.

Acceptance: host tests can register a fixture `slug.gpio` provider, resolve it
by module name, and reject an unregistered module through the ordinary checked
module error path.

### 2. Add minimal module contracts

Define the source-visible modules and foreign declarations for:

```slug
// slug.gpio
foreign write(name: str, level: bool): nil

// slug.time
foreign sleepMs(milliseconds: num): nil
```

Choose the repository's existing library-module convention when implementing
these declarations. `sleepMs` validates a finite, non-negative integral value
and an explicit upper bound before the host call. `write` accepts only a known
alias and a boolean.

Acceptance: frontend/module tests prove imported signatures, argument errors,
and unavailable-module diagnostics independently of ESP hardware.

### 3. Extend the frozen firmware generator

Replace the hello-only frozen backend shape incrementally with support for the
target program: imports, module references, booleans, strings, call results,
entry functions, and the tail-recursive blink function. Have the build step
consume a declared firmware capability manifest and fail before firmware
compilation if the source requests an unavailable module or alias.

Do not expose bytecode fields, create a generated standalone binary, or name
this output `.cslug`.

Acceptance: desktop tests compile the blink source to firmware build input;
negative fixtures reject an unlisted provider, unknown alias, and unsupported
source operation with a checked build diagnostic.

### 4. Implement a desktop GPIO simulation

Provide a test-only or desktop static-provider implementation that records
`write("led", level)` events and a deterministic delay abstraction. It exists
to prove source contracts and call order, not to emulate electrical behavior or
wall-clock timing.

Acceptance: a test observes `true, false` in order for one blink cycle and
proves that the firmware and desktop providers expose the same callable
surface.

### 5. Link the ESP32-S3 providers

At firmware startup, take ownership of the configured `esp-hal` GPIO pin and
create a push-pull output with a known initial inactive level. Register
`slug.gpio.write` to set that output level, applying board-profile polarity.
Register `slug.time.sleepMs` through the HAL's appropriate blocking delay
facility with the Stage 2 range checks preserved.

Keep the GPIO object inside host-owned state for the VM lifetime. Do not leak
an unbounded pin-selection or configuration API into Slug.

Acceptance: the selected target cross-builds, and a UART trace added only for
the proof reports the high/low calls in the same order as the desktop test.

### 6. Verify the physical blink

Flash the firmware, reset the board, and observe repeated LED transitions at a
documented cadence. Capture a matching serial trace where available, plus the
firmware image size, configured heap, and known stack limits.

Acceptance: the LED keeps flashing after boot for a documented observation
period; changing `sleepMs` in the Slug source changes the observed cadence
after rebuild and reflash.

## Explicit non-goals

This plan does not add raw pin access, input GPIO, interrupts, PWM, ADC,
networking, BLE, async timers, dynamic native loading, or `.cslug`. Those
capabilities need distinct ownership, memory, scheduling, and safety rules.

## Validation ladder

1. Module/signature and simulated-provider tests on the desktop.
2. Frozen-generator positive and negative tests.
3. ESP target `cargo check` and release cross-build.
4. Firmware unit or host tests for alias and delay range checks.
5. On-device UART transition trace and physical LED observation.

Run `make check` before handing off Rust changes; for this documentation plan,
validate links and run `git diff --check`.
