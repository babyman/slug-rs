# Clocks in `slug.time`, and sleeping through the scheduler

## Status

Accepted (2026-11-15). Prompted by Orble's first library feedback: sessions
read the time through SQLite's `unixepoch()` because Slug had no clock.

## Context

`slug.time` existed only as the embedded contract, exporting `sleepMs`, a
host function that blocked the whole VM. The desktop had no clock, and
`import("slug.time")` there resolved to that contract, with no
implementation behind it. Applications need two different clocks:
timestamps for people and other systems, which must follow the wall clock,
and durations and deadlines, which must not jump when the wall clock is set.

## Decision

`slug.time` exports:

- `nowMs()`: whole milliseconds since the Unix epoch on the wall clock.
  Desktop only; boards have no wall clock.
- `monotonicMs()`: whole milliseconds from an arbitrary origin, never going
  back. On the desktop it reads the clock `std::time::Instant` reads, which
  the VM's timers use; on a board, the timer the host gives the VM.
- `sleepMs(milliseconds)`: Slug code, `select { after milliseconds }`. It
  parks only the calling task. No host function blocks the scheduler to
  sleep.

The unit is in each name, as `sleepMs` already had it. The desktop module is
in `slug.core`, with the clocks in its C library; the embedded clutch keeps
its own contract, with `monotonicMs` as a board function.

## Consequences

- A duration measured on `monotonicMs` agrees with `after` timers.
- Programs that sleep on a board now need a VM built with `concurrency`; the
  ESP32-S3 firmware already is. The board no longer offers a blocking delay,
  and its 1000 ms sleep limit is gone with it.
- The static simulation host can no longer record sleep requests: it runs
  timers on the real clock, so its tests sleep briefly and check the elapsed
  time.
- Finer resolution than milliseconds, if benchmarking needs it, would be a
  separate function rather than fractional values from these.
