# A desktop native tier in its own crate

## Status

Accepted (2026-11-18). This is the "Decide" step of
[native-tier.md](../planning/native-tier.md), taken after Gate 0 measured
1.65x for a baseline compiler on Crawl's sight cast.

## Context

The interpreter is about 2.3x slower than CPython by geometric mean, and the
2026-11-17 ceiling round found no interpreter lever worth more than
single-digit percent. A hand-lowered Cranelift bound put a baseline
compiler at 5-10x the interpreter on numeric kernels and typed native code
within 1.2-1.4x of Java. Gate 0 measured 1.65x on data-heavy code.
`native-tier.md` records the measurements.

A native tier has to fit around these existing commitments:

- `slug-vm` is a pure runtime library that builds `no_std` for the
  ESP32-S3 ([crate organization](2026-09-26-crate-organization-and-import-loaders.md)).
  Cranelift does not target Xtensa, and the device has no room for it.
- `unsafe_code = "deny"` applies to the whole workspace. Entering
  generated code, and the pointers generated code passes back, cannot be
  safe Rust.
- The bytecode is private and unstable
  ([unstable bytecode surface](2026-08-30-unstable-rust-bytecode-surface.md)),
  and it is the one executable form the verifier checks
  ([bounded verification](2026-10-24-bounded-bytecode-verification.md)).
- Type annotations add no runtime checks, so native code cannot rely on a
  declared type.
- The native ABI that clutches use hides the VM's internals, and a
  compiler needs exactly those internals: `Value`'s layout and tags, frames,
  global slots, and the runtime's index, map, and reference-count paths.

## Decision

**A separate crate.** The native tier is a workspace crate,
`slug-native-tier`, that depends on `slug-vm` and on Cranelift
(`cranelift-codegen`, `-frontend`, `-jit`, `-module`, `-native`). It is not
a clutch and does not use the native ABI. The `slug` binary enables it with a
`native` feature. No other host does: embedded, the static and restricted
hosts, the REPL, and the slim and cooperative builds keep running only the
interpreter. Every program must behave the same with or without the native
tier.

**What `slug-vm` provides.** Behind a `native-hooks` feature, which
embedded builds never enable, `slug-vm` adds:

- a call and back-edge counter per chunk, and a hook trait the VM calls
  when a chunk reaches the compile threshold. The hook returns an entry
  point for the chunk, or declines;
- a way to call a compiled entry from the interpreter's call path, and to
  return to the interpreter from compiled code;
- the runtime operations compiled code calls. These are **safe Rust
  functions** over `&mut Value`, frame handles, and the VM, such as an index
  read, building a map, a merge, clone, drop, an arithmetic slow path,
  resolving a binding, a call, and checking the attention flag;
- pushing and popping a frame, as functions. Native code does not depend
  on the layout of `Frame`.

`slug-vm` gains no `unsafe` and no Cranelift dependency. These functions are
an internal contract between two crates of one workspace. They are
`#[doc(hidden)]`, may change in any commit, and are not part of the native
ABI or `native-abi.md`.

**Where the `unsafe` lives.** `slug-native-tier` holds the only exemption
from `unsafe_code`. It uses a module-level `#[allow(unsafe_code)]`, as
`slug-desktop-loader` and `slug-native-adapter` already do, limited to two
modules:

- the `extern "C"` shims that turn the pointers generated code passes into
  references and call `slug-vm`'s safe functions;
- the entry and exit between Rust and generated code.

The rest of the crate, including the IR, its lifter, and the lowering, is
safe Rust.

**A Slug IR lifted from bytecode.** The native tier compiles from its own
IR. The crate builds the IR from a chunk's verified bytecode when it
compiles that chunk; the frontend does not emit it, and `.cslug` artifacts
do not carry it. The IR is private to the crate and unstable, as the
bytecode is.

- It is SSA over basic blocks. Verified bytecode jumps only forward and
  loops only through `recur`, so a chunk's control flow is acyclic apart
  from one back edge to its entry.
- Where paths join with different stack heights, the values above the
  lowest height are dead, as the verifier treats them.
- Every value has a type. Each type is marked **proven** or **observed**:
  - proven: implied by the operation itself. A comparison gives a `Bool`,
    and `Int + Int` gives an `Int` or raises the overflow error. Proven
    types need no guard.
  - observed: taken from the runtime's type profile for that site. Each
    observed type is guarded, and a failed guard leaves native code.
- Every point where native code can leave (a guard, a helper that can
  fail, a call) carries a snapshot of the frame: which IR values go to
  which local and stack slot. Phase 3's deoptimization writes the
  registers back from this snapshot. Phase 1 records the snapshots without
  needing them.
- The checker's static types are not inputs. If the frontend later
  supplies facts the bytecode loses, such as "this local is never captured"
  or "this global is a `val`", they come as an optional side table on the
  chunk, checked by the verifier where a wrong fact would be unsound.

Phase 1 lifts into the IR and lowers it with every type unknown, which
reproduces Gate 0's boxed code. Phase 3 adds observed types and guards to
the same IR, so the front end is not replaced between the two.

**Type profiles.** The interpreter does not record types everywhere. When a
chunk reaches the threshold, it runs a short profiling phase that records
operand types at that chunk's sites before it compiles. Interpreted code
that never gets hot pays nothing.

## Consequences

- The `no_std` VM, the ESP32 build, and every host except the `slug`
  binary are unchanged. `make slim`, `make cooperative`, and the embedded
  check must keep passing without `native-hooks`.
- The release `slug` binary grows by about 2.5 MB and the lock file by about
  55 crates when `native` is enabled. Whether `native` is in the default
  build is decided at the end of Phase 1, on its measurements.
- The two engines must agree. The conformance corpus and `make
  test-clutches` run under both tiers in CI, including a mode that compiles
  every chunk on its first call. Phase 3 adds a mode that leaves native code
  at every guard.
- A change to a runtime operation's meaning lands once, in `slug-vm`, because
  compiled code calls the same function the interpreter does. Only the
  operations compiled inline (integer arithmetic and comparisons behind tag
  guards, truthiness, copies of uncounted values) exist twice, and the
  differential modes above cover them.
- Frames pushed by helper calls cost a little against a frame built
  inline. If a phase's measurements show that matters, exposing the layout
  of `Frame` under the feature is a later decision.
- `slug-vm` has to keep the hook functions safe. A function that would need
  `unsafe` to offer belongs in the shims instead.
- Functions using `defer`, `onerror`, `select`, or `fork` stay interpreted
  until Phase 4.
- Cranelift 0.113 on aarch64 has no PLT for the JIT, so shims are called
  through their addresses.

## Migration

None. Programs, bytecode, `.cslug` artifacts, and the native ABI are
unchanged.
