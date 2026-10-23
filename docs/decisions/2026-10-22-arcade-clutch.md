# The arcade clutch

## Status

Accepted

## Context

The next agent project is a Space Invaders game, to show that Slug can run a
real-time interactive program and to see whether an agent can write one. The
game's rules, state, movement, collisions, and scoring must be Slug; native
code may provide only what the hardware provided. The original game draws
into a 1-bit video RAM and finds collisions by reading it back, so the
faithful split gives Slug the video RAM and gives native code the display.

[Native interface profiles](2026-10-10-native-interface-profiles.md) leave the
pump hook and main-thread rules for game hosts open, and typed native
bindings cannot pass structs, which raylib's API uses throughout.

## Decision

- `slug.arcade`, in `slug.arcade.clutch`, is a `core` provider with a minimal
  API: open a display for 1-bit frames (optionally rotated for an upright
  monitor), colour overlay rectangles, present a frame, advance a 60 Hz frame
  clock, read named keys, and write screenshots. Its C wrapper calls raylib;
  no struct crosses the binding.
- The Slug program owns its loop. `frame()` blocks the whole VM until the
  next tick, so tasks and timers do not run between frames. This needs no
  pump hook; the CLI already runs the VM on the main thread, which macOS
  requires for windows. The pump decision stays open.
- A headless mode, selected by `SLUG_ARCADE_HEADLESS`, reads input from a
  script and can write frames, frame hashes, and PNG screenshots, so tests and
  agents can run a game with no window.
- raylib 5.5 is vendored, pruned to what the clutch builds, and compiled into
  a static library when the clutch is staged. A new manifest key,
  `native.link_args`, gives per-platform linker arguments: the static
  library's path inside the clutch, and system frameworks.

## Consequences

The clutch works only where raylib builds: macOS and Linux with X11 and GL
headers, which CI installs; Windows has no library entry yet. A program that
needs concurrent work alongside a frame loop will need the pump decision.
Staging the clutch takes a few seconds more, to build raylib.
