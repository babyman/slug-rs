# Embedded builtin contract

## Context

The ESP32-S3 host executes a closed artifact without the desktop frontend or
desktop native host. Its GPIO and time providers were registered, but the
ordinary `slug.builtin` native declarations were not. Consequently source that
used implicit `len` failed at runtime even though the same source ran on the
desktop host.

## Decision

The embedded Clutch supplies a source contract for `slug.builtin` containing
`Error`, `print`, `println`, and `len`. Firmware build and runtime hosts
register matching static native functions under that module name, so both
implicit globals and explicit `import("slug.builtin")` calls enter the closed
artifact graph.

`print` and `println` use the host diagnostic stream. `len` retains the VM
host behavior for strings, bytes, lists, and maps. `cfg`, `int`, and
`stacktrace` remain VM-provided.

## Consequences

Embedded programs may use the ordinary non-concurrent builtin surface without
source workarounds. The artifact limits account for the additional contract
module. `chan` and `close` remain absent until the separately planned embedded
concurrency profile; this decision does not add a partial scheduler.

## Migration

Replace embedded-only collection-count constants with `len` where they merely
duplicated a collection length. No bytecode or artifact compatibility promise
is introduced.
