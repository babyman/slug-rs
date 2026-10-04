# Slug ESP32-S3 firmware bootstrap

This standalone package proves the ESP32-S3 toolchain, bootloader, UART output,
and bounded `no_std + alloc` heap before linking the Slug frontend or VM. It is
not a member of the desktop Cargo workspace.

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

The serial monitor must contain:

```text
Slug ESP32-S3 firmware bootstrap ready
```

The next milestone replaces this Rust-only line with UART output driven by an
embedded Slug program. See
[`docs/planning/esp32-s3-proof-of-concept.md`](../../docs/planning/esp32-s3-proof-of-concept.md).
