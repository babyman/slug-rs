# FFI worker library leases

## Context

The experimental `slug-ffi-prototype/0.13` ABI permits a native library to
create a producer capability and use it from a detached C thread. That worker
can call `producer_destroy` while it is still returning through instructions
mapped from the same dynamic library. Releasing the final dynamic-library
lease at that call can make `dlclose` unmap the worker's return path, causing a
host process crash on platforms that unload immediately.

The ABI has no callback through which a native worker can report that it has
fully stopped executing library code.

## Decision

Once an ABI 0.13 native library creates a producer capability, the host retains
one dynamic-library lease for the rest of the process. Ordinary native
libraries that never create a producer retain their existing deterministic
shutdown and unload behavior.

## Consequences

Detached native producers remain safe to destroy from their final library-frame
operation. A process may retain one loaded-image lease for every native library
that has created a producer; the experimental ABI cannot reclaim those leases
reliably. A future native ABI may replace this policy with explicit
worker-quiescence ownership. For owned text and byte producer sends, the host
finalizes the C buffer after accepting the send but before publishing its copied
value, so a receiver cannot observe a value before its ownership callback.

## Migration

None. Existing native modules need no source, ABI, or behavior changes.
