# Signals on channels

## Status

Accepted

## Context

A Slug program has no way to learn that it is being asked to stop. Ctrl-C or
SIGTERM ends the process at once, so a server cannot close its listener,
finish what it is writing, or save its state. Crawl, which saves its world
to SQLite every few seconds, loses whatever happened since the last save.

A shutdown hook, a callback run at exit, was considered and rejected:

- A signal arrives at any moment, possibly in the middle of a VM step. Slug
  code cannot run in a signal handler, so a hook would have to wait for the
  scheduler anyway.
- A hook runs outside the program's own flow: `defer`s, open streams and
  nurseries would not unwind in order, and a hook that hangs or throws has no
  good outcome.

## Decision

Signals arrive as messages on a channel, through a module in the core
Clutch, `slug.os.signal`, as standard input does through `slug.io.stdin`.

- `notify(["int", "term"])` returns a new buffered channel that receives
  `"int"` for SIGINT (Ctrl-C on Windows) and `"term"` for SIGTERM (Ctrl-Break
  on Windows), in place of their default action. Shutdown is then ordinary
  code: a task receives the message and closes what the program serves, and
  `main` returns, running its `defer`s.
- The native side is a typed `producer` binding. On POSIX the handler only
  puts back the default action and writes the signal's number to a pipe; a
  watcher thread reads it and sends the name to every channel that asked.
  On Windows a console control handler, which runs on a thread of its own,
  sends directly.
- After one signal of a kind is delivered, the next takes the default
  action, so a stuck shutdown can always be ended with a second Ctrl-C.
  Calling `notify` again re-arms that kind.
- A signal that reaches no open channel takes the default action, as if no
  one had asked.
- The VM is unchanged. Signals are a host capability, and the embedded
  targets, which offer only the `core` native profile, do not have the
  module.

## Consequences

- Programs that do not import the module behave as before.
- Ending still follows structured concurrency: the root nursery waits for
  every spawned task, so a program has to stop the tasks it started, such as
  connection tasks holding open streams. Crawl's web server now ends its
  pages' event streams when its listener closes.
- Only the two signals that ask a program to stop are supported. SIGHUP,
  SIGUSR1 and the like can be added as further names if a program needs them.
- The Windows code is built and exercised only by CI's native job; the
  signal tests themselves run on POSIX.

## Migration

None.
