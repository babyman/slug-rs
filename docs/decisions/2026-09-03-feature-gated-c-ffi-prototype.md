# Contain the C FFI prototype in one unsafe module

## Context

The Rust runtime forbade unsafe code everywhere. A dynamic C module experiment
necessarily needs dynamic-loader calls, C function pointers, and opaque raw
call-context pointers. Spreading those operations into the VM or relaxing the
policy globally would make an experimental ABI contaminate the checked runtime
boundary it is meant to validate.

## Decision

The crate denies unsafe code by default and permits it only in the
`ffi-prototype` feature-gated module. That module owns all dynamic loading,
descriptor validation, and raw call-context conversion. It exposes a safe,
small loader API to the existing native-function facade. A C header and C
fixtures are test artifacts, not published ABI declarations.

The initial module supports only scalar integer and float arguments/results,
exact non-overlapping arities, and synchronous callbacks. Loaded libraries are
intentionally retained for the process lifetime.

## Consequences

- The VM, values, resources, and scheduler remain free of unsafe operations.
- The prototype tests concrete C layout, version rejection, and checked error
  propagation without promising ABI version 1 compatibility.
- The C fixture cannot test resources, channels, compound values, or raw C
  signature adaptation; those remain later ABI work.
- New unsafe FFI behavior must remain in the feature-gated boundary or receive
  a separate design decision.

## Migration

None. The feature is disabled by default and no external module is supported.
