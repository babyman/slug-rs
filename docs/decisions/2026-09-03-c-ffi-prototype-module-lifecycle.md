# Give C prototype modules explicit state ownership

## Context

The C math fixture proved a stateless callback boundary, but it did not answer
how a native module owns configuration, connection pools, or other host state.
Loading the same library repeatedly also opened unrelated dynamic-library
handles, while the native ABI direction requires code to remain resident rather
than be unloaded while callbacks or resources could still reference it.

## Decision

The prototype module entry receives an out-pointer for one opaque module-state
value. Every C callback receives that value. A module descriptor optionally
registers a teardown callback; non-null state requires that callback. The
runtime invokes it after the last Rust module owner releases the state.

The host canonicalizes library paths and keeps one `Arc`-owned library handle
in a process-lifetime registry. The feature-gated bridge uses a platform-loader
trait; the current implementation is macOS-only, while ABI-facing module logic
does not contain platform loader calls.

## Consequences

- C modules can own state without exposing pointers to Slug values.
- State teardown happens independently for each loaded module instance while
  its code remains resident.
- A later platform loader can implement the same trait without changing module
  descriptor or callback handling.
- The registry intentionally grows for the process lifetime; unloading remains
  outside the prototype and version 1.

## Migration

The unstable prototype entry point now accepts a module-state out-pointer, and
callbacks accept that state as a final argument. Modules that return non-null
state must provide `destroy_module`.
