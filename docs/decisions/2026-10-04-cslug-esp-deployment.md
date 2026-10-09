# Keep `.cslug` off the ESP32-S3 deployment path

## Context

Version-1 `.cslug` artifacts are desktop persistence and transport files. The
ESP32-S3 proof instead compiles a selected source program into a private Rust
flash image. Its current loader reads static slices embedded in firmware and
does not own filesystem, network, or other artifact storage.

The two representations are not comparable deployment candidates yet. The
frozen image has no runtime byte-stream decode: its constants and instructions
are linked as typed static data. `.cslug` would require a new byte-stream
reader, owned artifact storage, allocation budget, closed-graph host, and
boot-time decode on the device. Consequently its device size, decode time,
RAM use, and boot-to-output time are not measured values; treating desktop
codec measurements as ESP measurements would be misleading.

## Decision

The ESP32-S3 firmware does not accept `.cslug`, including artifacts produced
by a trusted build. It continues to accept only its private frozen image from
firmware flash. The existing frozen-image limits remain the relevant deployed
size, RAM, and boot-time measurements.

No device storage, Clutch module artifacts, memory mapping, zero-copy
execution, or replacement of the frozen-image boundary is introduced by
version 1. A future firmware-artifact proposal must first specify a device
storage authority, an ESP resource profile, the exact decode and allocation
measurements for the same blink workload, and untrusted-input validation.

## Consequences

Desktop `.cslug` compatibility remains useful without expanding firmware
authority or boot work. The firmware image remains coupled to its exact build
and VM revision, while `.cslug` continues to be validated at desktop loading
boundaries. The two formats must remain documented and tested as distinct.

## Migration

None. Existing firmware images and the ESP32-S3 proof remain unchanged.
