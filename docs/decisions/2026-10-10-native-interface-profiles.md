# Native extension interface profiles

## Context

Slug is meant to scale from microcontrollers to servers, not perfectly but
well. The native extension boundary has so far been shaped by two ends of that
range separately:

- The desktop clutch path loads C libraries with `dlopen` through the version
  0.13 prototype ABI. Its host table has roughly forty callbacks covering
  scalars, text, bytes, lists, maps, resources, channels, and thread-safe
  producers. That surface is sized for desktop and server hosts, and every
  argument and result crosses it through an indirect call.
- The embedded path registers Rust `NativeModule` providers statically through
  `ArtifactHost::install_static_providers`, which already rejects missing,
  extra, or duplicate providers against the artifact's `foreign`
  declarations. It uses only synchronous scalar calls and has no concurrency.

Further pressure is arriving from both directions. Board support (I2C, SPI,
UART, PWM, interrupts, radios) needs owned resources, events, and finite
memory. Games and servers (for example a raylib binding) need bulk data, a
host-owned main loop, and a call cost low enough for per-frame use. If each
need extends the prototype ABI, the smallest hosts inherit the largest
surface. Separately, `slug-vm` is `no_std` only when built for
`target_os = "none"`, which CI exercises only through the ESP32-S3 firmware
check and therefore only with that board's toolchain.

The existing decisions on the [opaque native extension interface](2026-08-23-opaque-native-extension-interface.md),
the [declared foreign registry](2026-08-24-declared-foreign-registry.md), and
[static Clutch providers](2026-10-04-static-clutch-providers.md) fix the
boundary's shape and its source-visible identity. They do not say which
capabilities a given host must offer, how a provider states what it needs, or
what a native call costs.

## Decision

**Source contract first.** The `foreign` declaration in Slug source remains the
authority for a native function's module, name, arity, and Slug-visible
parameter and result annotations. Native symbol bindings live in the clutch
manifest, as specified in [Typed native calls](2026-10-10-typed-native-calls.md).
Implementation language and linking (static Rust, static C, dynamic C) are
per-host choices behind the declaration.

**Two additive profiles.** The native extension interface is organized into
profiles. Each profile strictly extends the previous one.

| Profile  | Adds | Intended hosts |
|----------|------|----------------|
| `core`   | Synchronous calls; nil, bool, integer, and float arguments and results; borrowed read-only text and bytes arguments; text results copied on return; structs passed by copy with a declared layout; typed resources and buffer resources; checked structured errors. No channels, threads, or callbacks into Slug. | Every host, including `no_std + alloc` firmware with static linking |
| `events` | A native producer capability for a channel the Slug program created, with non-blocking send, closed-state, and close operations. A `threads` qualifier additionally permits calls from foreign threads. | Hosts with a polled loop; `threads` only where threads exist |

Lists and maps are not part of the native interface. Native code speaks
scalars, text, bytes, structs, resource handles, and buffers; Slug library code
builds compound values from them. A new profile is added by decision record,
not by extending an existing one in place.

**Providers declare, hosts enforce.** A provider states the highest profile it
needs (and `threads` if applicable). A host that does not offer that profile
rejects the provider at build time (static) or load time (dynamic) with a
checked error. A missing capability is never discovered as an absent symbol
during execution.

**Host-driven progress.** The common execution model for every profile is that
the host owns the loop and advances the VM (`Vm::poll`). A microcontroller main
loop or interrupt, a game frame, and a server event loop are all instances of
it. A blocking provider call (such as the embedded `sleepMs`) is permitted only
where the host documents that it stalls the whole VM. The pump interface itself
is a separate record.

**Scale-out by isolation.** A VM instance is single-threaded. Server-side
parallelism is by multiple isolated VMs exchanging owned send values, not by
sharing `Value`s across threads, with per-VM memory and instruction limits as
the unit of control. This record adopts that direction; it does not implement
it.

**Cost is opt-in.** Capabilities above `core` compile out of builds that do not
select them, using the existing Cargo features and `cfg` mechanism rather than
runtime flags. CI keeps building `slug-vm` for a bare-metal target (today
through the ESP32-S3 firmware check) so that the smallest configuration cannot
regress silently; a board-independent bare-metal check of the `core`
configuration is worth adding when a second target exists.

**One contract, two linking forms.** Static linking (embedded, and a game
shipped as one executable) registers symbols from the manifest at build time.
Dynamic loading (desktop and server clutches) resolves the same symbols with
`dlopen`. Rust `NativeModule` providers remain a supported static form for
hosts that implement a module in Rust.

**Prototype ABI.** The 0.13 prototype C ABI is deprecated and frozen: it
receives no new features. It is replaced by typed native calls and the
`events` producer interface, and removed once the first-party clutches have
moved. The VM-facing concepts in `docs/reference/native-abi.md` (opaque
call-scoped values, structured errors, resource lifecycle, producer capability)
remain authoritative; only the C binary representation changes.

## Consequences

The embedded GPIO and time providers are `core` providers, and the existing
static registration path is their reference implementation. A raylib-class
binding is a `core` provider that additionally needs the host-loop decision
below.

A provider that needs only scalar calls runs unchanged on a board, a desktop,
and a server host, because the contract and the profile it needs are stated
once.

Intentionally unsupported by this record:

- the pump hook and the main-thread and blocking rules for game or windowing
  hosts;
- shared pin and peripheral ownership across providers (the descriptor is the
  likely place for a static wiring ledger);
- loading artifacts at runtime on constrained hosts, which changes the trust
  model and requires its own record; and
- any change to source syntax, bytecode, or `.cslug`.

Documentation and tests that must stay aligned: `docs/reference/native-abi.md`
needs a profile section and a note that the C binary form is superseded, and
`NativeModule` registration should carry the required profile.

## Migration

Existing providers are classified retroactively: embedded providers as `core`,
desktop clutches as `core` plus `events`. No source, bytecode, or artifact
compatibility promise changes. The migration of native C clutches off the
prototype ABI is described in [Typed native calls](2026-10-10-typed-native-calls.md).
