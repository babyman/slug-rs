# Slug ESP32-S3 VM proof of concept

This standalone package cross-compiles
[`programs/blink.slug`](programs/blink.slug) and the selected
`slug.embedded` source contracts through a desktop build dependency into a
closed version-1 `.cslug` artifact in `OUT_DIR`. Firmware embeds, bounds-checks,
and executes that artifact through the shared `no_std + alloc` VM on an
ESP32-S3. It owns push-pull GPIO 2 and GPIO 4 outputs plus a blocking delay
provider for the active-high `led` and `led4` aliases; it links neither the source
frontend nor a desktop loader. It is not a member of the desktop Cargo
workspace.

The `led` alias is the PinPulse Shield indicator on GPIO 2 (active high), and
`led4` is the second active-high host-selected output on GPIO 4. Source code
never selects a raw GPIO pin.

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

The serial monitor repeatedly prints the logical transitions:

```text
Slug GPIO led -> low
Slug GPIO led -> high
```

The program requests each level for 250 ms before tail-recurring, so the LED
should blink with a 500 ms period. See
[`docs/planning/esp32-s3-static-clutch-gpio.md`](../../docs/planning/esp32-s3-static-clutch-gpio.md).

## Hardware verification

Verified on 2026-10-08 with the Lonely Binary ESP32-S3 N16R8 and PinPulse
Shield over `/dev/cu.usbmodem1101`:

- `espflash` identified an ESP32-S3 revision 0.2 with 16 MB flash and wrote a
  372,848-byte application image (2.28% of the factory partition).
- The firmware built from the embedded closed `.cslug` artifact and UART
  repeatedly reported `Slug GPIO led -> low`, `Slug GPIO led4 -> low`, then
  their corresponding high transitions.
- The PinPulse Shield GPIO 2 indicator was visibly blinking at the expected
  250 ms-on / 250 ms-off cadence.
- The configured bounded heap is 73,744 bytes. The release ELF reserves
  331,720 bytes for `.stack`; peak heap use is not yet instrumented.
