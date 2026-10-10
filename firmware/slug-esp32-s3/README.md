# Slug ESP32-S3 VM proof of concept

This standalone package cross-compiles
[`programs/blink.slug`](programs/blink.slug) and the selected
`slug.embedded` source contracts through a desktop build dependency into a
closed version-1 `.cslug` artifact in `OUT_DIR`. Firmware embeds, bounds-checks,
and executes that artifact through the shared `no_std + alloc` VM on an
ESP32-S3. It exposes a bounded set of safe, push-pull GPIO outputs and a
blocking delay provider; it links neither the source frontend nor a desktop
loader. It is not a member of the desktop Cargo workspace.

`slug.gpio.write(pin, level)` uses physical GPIO numbers. The PinPulse
profile permits the 29 outputs in its light-walk order: GPIO 4--7, 15--18, 8,
3, 46, 9--14, 1, 2, 42--38, 0, 45, 48, 47, and 21. Serial GPIO 43/44, native
USB GPIO 19/20, and PSRAM GPIO 35--37 remain unavailable. Outputs are
configured only on first write. The default program initializes every selected
output low, chases one LED at a time, lights all LEDs, and clears them.

## Prerequisites

Install Espressif's Xtensa toolchain and flasher, then make them available to
the shell:

```sh
cargo install espup espflash --locked
espup install --targets esp32s3
source /Users/evan/export-esp.sh
export PATH="$HOME/.cargo/bin:$PATH"
```

`export-esp.sh` adds `xtensa-esp32s3-elf-gcc` to `PATH`. The ESP Rust toolchain
needs that linker even for `cargo check` because this package verifies the
toolchain in `build.rs`.

Confirm the tools before building:

```sh
rustup toolchain list
xtensa-esp32s3-elf-gcc --version
espflash --version
```

## Build

From this directory, build the firmware with the Espressif compiler:

```sh
cargo +esp check
cargo +esp build --release
```

The release firmware is:

```text
target/xtensa-esp32s3-none-elf/release/slug-esp32-s3
```

## Install and monitor

Connect the board through its USB port, then find the serial device and verify
the chip. The verified board appears as `/dev/cu.usbmodem1101`; do not assume
that name is stable across hosts or reconnects.

```sh
ls /dev/cu.usbmodem* 2>/dev/null
espflash board-info --chip esp32s3 --port /dev/cu.usbmodem1101
```

Flash the release image:

```sh
espflash flash --chip esp32s3 --port /dev/cu.usbmodem1101 \
  target/xtensa-esp32s3-none-elf/release/slug-esp32-s3
```

Then reset and capture the UART proof. A separate non-interactive monitor is
more reliable than `flash --monitor` in non-terminal sessions:

```sh
espflash monitor --non-interactive --chip esp32s3 --port /dev/cu.usbmodem1101 \
  --elf target/xtensa-esp32s3-none-elf/release/slug-esp32-s3
```

The serial monitor prints the physical transitions, beginning with the
initialization pass:

```text
Slug GPIO 4 -> low
Slug GPIO 5 -> low
...
Slug GPIO 4 -> high
```

The chase holds each LED high for 80 ms, then shows the whole lap for 600 ms
and clears it for 300 ms before tail-recurring. See
[`docs/planning/esp32-s3-static-clutch-gpio.md`](../../docs/planning/esp32-s3-static-clutch-gpio.md).

## Hardware verification

The earlier two-pin proof was verified on 2026-10-09 with the Lonely Binary
ESP32-S3 N16R8 and PinPulse Shield over `/dev/cu.usbmodem1101`:

- `espflash` identified an ESP32-S3 revision 0.2 with 16 MB flash and wrote a
  374,368-byte application image (2.28% of the factory partition).
- The firmware built from the embedded closed `.cslug` artifact and UART
  repeatedly reported ordered GPIO 2 and GPIO 4 low/high transitions.
- The PinPulse Shield GPIO 2 indicator was visibly blinking at the expected
  250 ms-on / 250 ms-off cadence.
- The configured bounded heap is 73,744 bytes. The release ELF reserves
  331,720 bytes for `.stack`; peak heap use is not yet instrumented.

The current light-walk program has a desktop artifact-host regression that
proves it reaches its first GPIO callback. Reflash it and use the monitor above
to complete its separate 29-output on-device observation.
