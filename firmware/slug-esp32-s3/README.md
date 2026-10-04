# Slug ESP32-S3 VM proof of concept

This standalone package cross-compiles
[`programs/blink.slug`](programs/blink.slug) through a desktop build dependency
into a small firmware-resident program image, then loads it into Slug's shared
VM on an ESP32-S3. It owns a push-pull GPIO 2 output and a blocking delay
provider for the PinPulse Shield's active-high `led` alias, uses a bounded
`no_std + alloc` heap, and intentionally links neither the source frontend nor
a desktop loader. It is not a member of the desktop Cargo workspace.

The build validates the blink source against [`capabilities.rs`](capabilities.rs)
and emits its private frozen image. The `led` alias is the PinPulse Shield
indicator on GPIO 2 (active high); source code never selects a raw GPIO pin.

## Prerequisites

Install Espressif's Xtensa toolchain and flasher, then make them available to
the shell:

```sh
cargo install espup espflash --locked
espup install --targets esp32s3
source "$HOME/export-esp.sh"
export PATH="$HOME/.cargo/bin:$PATH"
```

## Build and flash

From this directory, build the firmware with the Espressif compiler:

```sh
cargo build --release
```

With the board connected over USB, flash and monitor it:

```sh
espflash flash --monitor --chip esp32s3 target/xtensa-esp32s3-none-elf/release/slug-esp32-s3
```

The serial monitor repeatedly prints the logical transitions:

```text
Slug GPIO led -> high
Slug GPIO led -> low
```

The program requests each level for 250 ms before tail-recurring, so the LED
should blink with a 500 ms period. See
[`docs/planning/esp32-s3-static-clutch-gpio.md`](../../docs/planning/esp32-s3-static-clutch-gpio.md).
