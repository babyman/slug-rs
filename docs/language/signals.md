# Signals

The `slug.os.signal` module, from the core Clutch, turns the operating
system's requests to stop into messages on a channel, so a program can stop
the way it chooses.

```slug
val signal = import("slug.os.signal")
val tcp = import("slug.net.tcp")

val main = fn() {
  val listener = tcp.listen("127.0.0.1", 8080)
  val stop = signal.notify(["int", "term"])
  spawn { select { recv stop }; tcp.close(listener) }
  serveUntilClosed(listener)
  saveWork()
}
```

`notify(names:list):chan<str>` returns a new channel that receives `"int"` for
each interrupt and `"term"` for each request to terminate, of the kinds in
`names`, in place of their default action, which ends the program. An
interrupt is SIGINT, sent by Ctrl-C in a terminal; on Windows it is Ctrl-C. A
request to terminate is SIGTERM; on Windows it is Ctrl-Break. Any other name
throws an `Error` of type `signal.name`, and a host that cannot watch for
signals throws one of type `signal.error`.

Every channel asking for a kind receives each signal of that kind. The
channel buffers a few signals; a signal that finds it full is not queued
again, since one is already waiting.

- A program that never calls `notify` ends on these signals as before.
- Once a signal has been delivered, the next one of the same kind takes the
  default action and ends the program at once, however busy it is, so a
  stuck shutdown can always be cut short. Calling `notify` again for that
  kind asks for the next one too.
- A signal that reaches no open channel, because every channel that asked
  for it has gone, also takes the default action.

Receiving the message changes nothing by itself: the program decides what to
do. To end, it finishes its work and returns from `main` as usual, running its
`defer`s; the root nursery still waits for every spawned task, so a program
stops the tasks it started, for example by closing a listener or channels.

The module needs the core Clutch's native library, as `slug.io.stdin` does,
and is not available on embedded targets.
