# Embedded host capabilities

## Context

Slug's desktop executable assembles a VM with filesystem-backed import
resolution, process configuration, standard output, and optional Clutch native
loading. Those are host choices, not requirements of bytecode execution. An
ESP32-S3 proof of concept needs to run a small source program from firmware
flash without a filesystem or operating system.

[Host-owned module resolution](2026-09-23-host-owned-module-resolution.md)
already reserves a ROM-backed resolver for this use. [Crate organization and
import loaders](2026-09-26-crate-organization-and-import-loaders.md) further
establishes that loaders supply imports and import-scoped activation only; they
do not own VM bootstrap or builtins. The current VM and frontend nevertheless
use `std` directly, so they cannot yet be linked into bare-metal firmware.

## Decision

The initial embedded target is a bare-metal ESP32-S3 firmware host. It will use
`no_std` with Rust's `alloc` library and an explicitly initialized bounded heap.
`alloc` provides heap-backed Rust types such as `Vec`, `String`, and `Rc`; it
does not supply filesystem, environment, process, standard-stream, or desktop
dynamic-library services.

The VM, bytecode validation, source semantics, and checked error behavior are
shared across desktop and embedded hosts. A target may vary only in explicitly
provided host capabilities and platform availability. Cargo feature boundaries
are limited to implementation constraints such as `std` versus `no_std +
alloc` and the selected concurrency runtime; they must not create a separate
embedded source-language dialect or silently relax checked failures.

The embedded host will compose the frontend and VM with these responsibilities:

- a resolver maps a fixed, compiled-in module name to uncompressed Slug source
  stored in firmware flash; any other import reports the ordinary checked
  missing-module failure;
- the host registers the native capabilities explicitly granted to firmware;
- the first granted capability is `println`, which writes its checked value to
  the configured UART;
- the bootstrap initializes startup state, the allocator, UART, host, and VM;
  it compiles the embedded entry source and invokes `main()`.

The resolver does not register general builtins or control hardware. A pure
Slug source module may come from its table, but UART, GPIO, Wi-Fi, and other
device services are native host capabilities registered outside the resolver.
This preserves the existing rule that source resolution cannot silently grant
host authority.

The proof of concept excludes external storage, configuration from the process
environment, Clutches, dynamic native libraries, networking, and Slug
concurrency. It embeds source rather than private VM bytecode; the future
portable `.cslug` format remains the only intended compiled-artifact contract.

Each firmware boot creates a fresh host and VM. A later soft-reset facility
must either recreate that state or prove that it releases all VM-owned roots;
it must not leave host or program values accidentally reachable across runs.

## Consequences

### Positive

The firmware has a small, auditable authority boundary. It can execute the
same source compilation and checked-runtime path as other hosts while exposing
only UART output. Flash-resident source avoids a filesystem, package format, or
premature private-bytecode serialization commitment.

Making `slug-vm` and the required frontend path work with `no_std + alloc`
creates a clear boundary between portable runtime code and desktop services.
The fixed heap makes memory use measurable and turns resource limits into an
explicit host policy.

A small embedded-core regression target can detect desktop-service leakage
without requiring physical hardware in every test run.

### Negative

Direct `std` uses must be moved to `alloc`, replaced by an embedded-compatible
collection, or isolated behind a `std` feature. The firmware must select and
initialize an allocator, handle allocator exhaustion deliberately, and measure
flash, static RAM, stack, and heap use. ESP32-S3 builds require Espressif's
Xtensa Rust toolchain in addition to the repository's desktop Rust toolchain.

The initial source-at-boot design consumes boot time and heap space for parsing
and lowering. It is intentionally a functional baseline, not a size or
startup-time optimum.

The shared-core rule requires continued cross-target testing. A platform
capability that needs a new VM semantic must be designed explicitly instead of
being hidden behind an embedded-only feature.

### Neutral

This decision narrows the embedded-host portion of
[Host-owned module resolution](2026-09-23-host-owned-module-resolution.md).
It does not alter Slug syntax, desktop import precedence, the native ABI,
private bytecode stability, or the portable `.cslug` contract. It does not add
a garbage collector; the existing reference-counted value model remains the
starting point.

## Migration

None for Slug source programs or desktop executables. Rust embedding APIs are
pre-release and may gain `std` feature boundaries or embedded host assembly
types while preserving the documented desktop behavior.
