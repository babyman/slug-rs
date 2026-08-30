# Make VM execution metrics opt-in

## Context

The VM benchmark counts instructions, source-span clones, frame creation, and
local binding-cell allocation. Updating these counters in ordinary dispatch
adds mutable bookkeeping to the path whose performance the project is trying
to measure.

## Decision

Execution-path VM metrics are available only through a default-disabled Cargo
feature named `metrics`. Without that feature, the VM does not store metric
state and counter updates compile out. The benchmark target requires the
feature, and `make bench-vm` enables it.

Program-layout measurements remain explicit `Program` queries rather than VM
execution counters, so they do not require the feature.

## Consequences

Normal builds and ordinary performance measurements avoid metric bookkeeping.
Benchmark and representation tests that need runtime counters must enable
`metrics`. `VmMetrics` and `Vm::metrics` are unavailable without it; they are
private-representation diagnostics rather than a stable embedding API.

## Migration

None for Slug source programs or bytecode. Rust callers that use VM metrics
must enable the `metrics` Cargo feature.
