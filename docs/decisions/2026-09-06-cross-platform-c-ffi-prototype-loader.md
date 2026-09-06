# Load C FFI prototype modules on supported CI platforms

## Context

The test-only C FFI prototype originally used macOS loader symbols and fixture
compiler flags unconditionally. That prevented the same checked loader
boundary from building or running in Linux and Windows workflows.

## Decision

Keep dynamic loading inside the feature-gated prototype module, with platform
adapters for `dlopen`/`dlsym` on Unix and `LoadLibraryW`/`GetProcAddress` on
Windows. Build C fixtures as native shared libraries using the target's DLL
prefix and suffix. Windows runs the portable loader tests; fixtures requiring
SQLite or POSIX threads remain Unix-only test coverage.

## Consequences

- The experimental loader is tested on macOS, Linux, and Windows CI runners.
- Windows library paths use UTF-16 and fixture exports use `__declspec(dllexport)`.
- The stable runtime remains free of dynamic-loader unsafe code and the C ABI
  remains an explicitly unsupported compatibility promise.

## Migration

None. The feature remains disabled by default.
