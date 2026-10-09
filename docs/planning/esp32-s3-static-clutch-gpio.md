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
- The firmware build resolves the selected source contracts on the desktop and
  writes one closed version-1 `.cslug` program artifact. The device decodes
  only that bounded, checked graph and explicitly registered providers.
- `sleepMs` is synchronous for this first path. It does not enable the
  scheduler, `after`, or background interrupt producers.
- The device never resolves source, Clutches, module artifacts, files, or
  dynamic native libraries at runtime.

## Delivery stages

### 0. Identify the board LED safely

The selected board is the Lonely Binary ESP32-S3 N16R8 / ESP32-S3-DevKitC-1
V1.6. Its onboard LED is an addressable RGB LED on GPIO 48 (`RGB@IO48`), not a
plain digital output. A boolean GPIO write cannot make it blink: it requires a
WS2812-compatible encoded waveform, normally emitted through a timing-capable
peripheral such as RMT.

This board therefore cannot use `slug.gpio.write("led", level)` for its
onboard LED. The GPIO alias remains valid for a separately wired plain LED,
but the onboard target requires an explicit RGB/LED capability before the
hardware stages can proceed. GPIO 48 is reserved for that onboard LED profile;
Slug source continues to name no raw GPIO number.

Acceptance: the board identity and LED protocol are recorded from the vendor
documentation, and the plan does not incorrectly treat the RGB LED as a direct
GPIO output.

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

### 3. Compile a closed firmware artifact

Compile the target program and its selected source contracts into a closed
version-1 `.cslug` graph in `OUT_DIR`. Have the build step fail before firmware
compilation if source imports a provider outside the static resolver.

Embed the artifact as immutable flash bytes and decode it under explicit device
limits before starting the VM-owned artifact host.

Acceptance: desktop tests encode/decode the blink source under the device
limits; the build script writes the same graph as firmware build input; and an
unlisted provider remains a checked build diagnostic.

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
networking, BLE, async timers, dynamic native loading, or version-2 importable
module artifacts. Those
capabilities need distinct ownership, memory, scheduling, and safety rules.

## Validation ladder

1. Module/signature and simulated-provider tests on the desktop.
2. Frozen-generator positive and negative tests.
3. ESP target `cargo check` and release cross-build.
4. Firmware unit or host tests for alias and delay range checks.
5. On-device UART transition trace and physical LED observation.

## Stage 6 verification record

On 2026-10-04, the `slug-esp32-s3` release image was flashed to the Lonely
Binary ESP32-S3 N16R8 / PinPulse Shield through `/dev/cu.usbmodem1101`.
`espflash` identified an ESP32-S3 revision 0.2 with 16 MB flash and reported a
290,944-byte application image (1.78% of the factory partition). UART emitted
repeated `Slug GPIO led -> high` and `Slug GPIO led -> low` transitions, and
the PinPulse Shield's active-high GPIO 2 indicator was visibly observed
blinking. The program uses the selected 250 ms on/off delays, for a 500 ms
nominal period.

The current firmware reserves a 73,744-byte bounded heap and a 331,720-byte
`.stack` section. Peak heap use is not yet instrumented; that measurement is a
follow-up before increasing the frozen program's complexity.

Run `make check` before handing off Rust changes; for this documentation plan,
validate links and run `git diff --check`.
