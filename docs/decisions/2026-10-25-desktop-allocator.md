# Use mimalloc in the desktop executable

## Status

Accepted

## Context

Profiles of the source benchmark workloads show the platform allocator
(`_xzm_free`, `_xzm_xzone_malloc` on macOS) among the hottest symbols: values
and collections allocate constantly. Dependencies must remove more owned
complexity than they create, and an allocator removes none, so it needs a
measured case.

Interleaved A/B runs of profile-guided builds against `90a47cd`, seven
rounds each, on macOS:

- mimalloc (v3, the `mimalloc` 0.1.52 default): tuned geomean 0.92x,
  hold-out 0.89x. `crawl-game` 0.90x, `http-server` 0.87x, `defer-errors`
  0.76x. `shared-append` regressed 1.24x.
- mimalloc v2: similar, but `shared-append` 1.38x.
- jemalloc (`tikv-jemallocator`): tuned 0.94x, hold-out 0.93x, no
  regression. It does not support Windows MSVC, which CI builds.

The `shared-append` regression came from `List::append` copying a shared
list at its exact length and then growing it: the platform allocator grew
the large block in place, while mimalloc copied it again.

## Decision

The `slug` executable sets `mimalloc::MiMalloc` as its global allocator. The
VM, frontend, and loader crates do not choose an allocator, so embedders and
the `no_std` ESP32 firmware keep their own.

## Consequences

The desktop executable builds mimalloc's C sources, which needs a C
compiler on every CI platform. Allocation-heavy programs run faster, and
peak memory is mostly lower. A list copy that relies on the allocator
growing a block in place must instead copy once with room to grow.

## Migration

None.
