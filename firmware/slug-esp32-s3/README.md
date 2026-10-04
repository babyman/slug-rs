# Slug ESP32-S3 VM proof of concept

This standalone package runs a direct bytecode program through Slug's shared VM
on an ESP32-S3. It uses a bounded `no_std + alloc` heap and intentionally links
neither the source frontend nor a desktop loader. It is not a member of the
desktop Cargo workspace.

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

The serial monitor repeatedly prints:

```text
Slug VM result: 42
```

The next milestone introduces an embedded native `println` capability and a
flash-resident program loader. See
[`docs/planning/esp32-s3-proof-of-concept.md`](../../docs/planning/esp32-s3-proof-of-concept.md).
