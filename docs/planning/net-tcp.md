# TCP networking: `slug.net.tcp`

## Purpose

Give Slug programs TCP servers and clients, so the next agent project can be
an HTTP/1.1 server library for building web apps. The library is native, so
we build it; agents then write the HTTP layer in Slug on top of it, and the
best run becomes the standard HTTP module, as `slug.fmt.json` did.

## Scope

Version 1:

- TCP over IPv4 and IPv6: listen, accept, connect, read, write, half-close,
  close.
- Many connections at once, each served by its own Slug task, without one
  connection's blocking I/O stalling the others.
- macOS and Linux.

Not in version 1: TLS, UDP, DNS beyond what `getaddrinfo` does inside
`connect` and `listen`, Windows, and embedded targets. ESP32 can later offer
the same API over lwIP in the embedded clutch.

## How blocking I/O stays off the scheduler

The VM runs Slug tasks on one thread, so a native call that blocks in
`accept` or `recv` stalls every task. `slug.io.stdin` already solves this for
reading: a native thread does the blocking reads and sends each line into a
channel the Slug program created, through a producer capability
([typed native producers](../decisions/2026-10-11-typed-native-producers.md)).
A task waiting on that channel with `select` parks, and the others run.

`slug.net.tcp` uses the same mechanism:

- **Accepting.** `listen` starts an accept thread that sends each accepted
  connection into the listener's channel.
- **Reading.** Each connection has a reader thread that `recv`s into a buffer
  and sends each chunk as `bytes` into the connection's channel, closing the
  channel at end of stream or on error.
- **Backpressure.** When a channel is full, the native thread retries after a
  short pause, as the stdin reader does, and stops calling `recv`. TCP flow
  control then slows the peer, so a slow Slug consumer never buffers
  unboundedly.

### Handing over a connection

A producer can send only scalars, text and bytes, not resource handles. The
accept thread therefore queues each accepted socket inside the listener and
sends only a tick, `0`, into the listener's channel. A task that receives a tick
calls `adoptRaw(listener)`, which moves the oldest queued socket into a new
connection resource. Ticks and queued sockets are matched one for one, so any
number of tasks can accept from one listener. So:

- Slug code never holds a number that names a socket, so it cannot take over
  another one, such as standard input.
- Closing the listener closes every socket still queued, so an accepted
  connection that was never adopted does not leak.
- A fatal accept error is sent as a negative tick, `-errno`, and then the
  channel closes.

`connect` uses a connecting resource of its own: a connect thread resolves the
host and connects, records the socket or the error in it, and sends the status
into a one-slot channel. The task waiting on that channel then calls
`finishConnectRaw` to take the socket.

### Types

`Listener` and `Connection` are structs that pair the native handle with its
channel, exported as type aliases over private schemas, so programs write
`tcp.Connection` but cannot construct one.

### Lifetimes

A connection's native state is shared by Slug, which holds the `Connection`
resource, and the reader thread. It is reference-counted:

- releasing the resource, by `close` or when it is collected, calls
  `shutdown(SHUT_RDWR)`, which wakes a reader blocked in `recv`;
- the reader then closes its channel, releases its producer and drops its
  reference;
- the socket is closed when the last reference goes.

If the VM shuts down first, sends return `closed`, the threads stop, and
references drain the same way.

### Writing

A blocking `send` inside a native call would stop every task while a peer is
slow to read, and a first version that did so deadlocked an in-process echo:
the writing task held the scheduler while the reading task could not run.
`SO_SNDTIMEO` did not bound it either, because the timeout restarts after
every partial send.

Each connection therefore also has a writer thread. `write` copies the data
into a native queue, in a call that never blocks, and the writer sends from
the queue. While more than 256 KiB is queued, `write` parks its task: it asks
for a tick through the connection's one-slot `writable` channel, which the
writer sends as the queue shrinks, and throws `net.timeout` when the peer
takes nothing for the write timeout (5 seconds by default, settable per
connection). A failed send is reported by the next `write`.

`shutdownWrite` and `close` are queued behind the data: the writer shuts down
the sending side once the queue is empty, and `close` waits, parking its task,
for the writer to finish, bounded by the write timeout, so a response written
just before `close` is not lost when the program exits.

## API

```slug
val tcp = import("slug.net.tcp")

// A server: one task per connection.
val listener = tcp.listen("127.0.0.1", 8080)   // port 0 picks a free port
println(tcp.localPort(listener))
val serve = fn(connection) {
    defer tcp.close(connection)
    match tcp.read(connection, timeout = 5000) {
        nil => nil                               // the peer closed
        data => tcp.write(connection, data)      // echo one chunk
    }
}
val loop = fn() {
    match tcp.accept(listener) {
        nil => nil                               // the listener was closed
        connection => {
            spawn { serve(connection) }
            recur()
        }
    }
}
loop()
```

| Function | Result |
|---|---|
| `listen(host:str, port:num, backlog:num = 128):Listener` | A listening socket. |
| `localPort(listener:Listener):num` | The bound port, for `port = 0`. |
| `accept(listener:Listener, timeout:num\|nil = nil):Connection\|nil` | The next connection, or `nil` once the listener is closed. Parks only the calling task. |
| `connect(host:str, port:num, timeout:num = 10000):Connection` | A connected socket. |
| `read(connection:Connection, timeout:num\|nil = nil):bytes\|nil` | The next chunk, or `nil` at end of stream. |
| `chunks(connection:Connection):chan<bytes\|num>` | The connection's chunk channel, for a `select` over several sources; a number is a failed receive's error code. |
| `write(connection:Connection, data:bytes\|str):nil` | Queues all of `data`, waiting while more than 256 KiB is queued. |
| `setWriteTimeout(connection:Connection, milliseconds:num):nil` | How long the peer may take nothing before a write times out. |
| `shutdownWrite(connection:Connection):nil` | Half-close: the peer sees end of stream, and reading continues. |
| `close(value:Connection\|Listener):nil` | Closes it; closing twice does nothing. |
| `peer(connection:Connection):str` | The peer's address and port. |

All timeouts are in milliseconds. A failure throws an `Error` whose `type` is
`net.error`, `net.timeout`, `net.refused` or `net.address`, with the system's
message.

## Prerequisites

**Converting between bytes and strings.** Slug has no conversion between
`bytes` and `str`, and an HTTP parser needs both directions. Add
`string.fromBytes(data:bytes):str`, which throws `string.utf8` on invalid
UTF-8, and `string.toBytes(text:str):bytes`. Both would be natives in the core
clutch, like `ordRaw`, so this needs no VM change.

**Building up bytes.** Bytes are immutable. Measured on a release build,
appending 4 KiB chunks to a buffer only one binding holds reuses the buffer, so
it is linear: 16 MiB in 4,000 chunks takes 10 ms. A buffer that is also held
elsewhere, such as in a map field or a module `var`, is copied on each append,
which is quadratic: 4 MiB takes 0.13 s and 16 MiB about 2 s. HTTP requests are
far smaller than that, and a request body can be collected with a uniquely
held accumulator, so no `bytes.join` is needed yet. Revisit if the HTTP
project's runs show copying costs.

## Steps

Each step ends with `make check` passing and is one commit.

1. **Bytes and strings.** `string.fromBytes` and `string.toBytes`, with
   clutch tests, and the bytes-joining measurement. Done.
2. **Clutch skeleton.** `clutch/slug.net.tcp.clutch` with `listen`,
   `localPort`, `accept`, `connect` and `close`, the C source, staging in
   `scripts/stage-native-clutches.sh`, and an entry in `clutch/manifest.toml`.
   Done.
3. **Reading and writing.** The reader and writer threads, `read`, `chunks`,
   `write`, the write timeout, `shutdownWrite` and `peer`. Done.
4. **Robustness.** The tests below that cover failure: peer resets, closing a
   listener while a task waits in `accept`, closing a connection while its
   reader is blocked, VM shutdown with live threads, and 200 concurrent
   connections. Done.
5. **Documentation.** `docs/language/networking.md`, and the module in the
   library listing the eval prompts give agents.
6. **The HTTP project.** See below.

## Tests

All on loopback with port 0, so they run in parallel and never collide:

- **Rust integration tests** in `crates/slug/tests/typed_net.rs`, like
  `typed_sqlite.rs`, run Slug programs that serve and connect within one
  program using `spawn`, and check their output:
  - echo, including chunks larger than one `recv` buffer;
  - end of stream, and half-close in each direction;
  - a refused connection, and a bad address;
  - read and accept timeouts;
  - 200 clients at once, each echoing its own data;
  - backpressure: a client sends 64 MiB to a server that reads slowly, and
    memory stays bounded;
  - a blocked writer times out;
  - closing a listener with a task in `accept`, which then returns `nil`;
  - closing a connection with its reader blocked;
  - exiting with connections still open, which leaves no thread running.
- **Clutch tests** in Slug, run by `slug test`, for the Slug-level wrappers.
- **A Rust client** in some tests, so a Slug server is also checked against
  behaviour Slug did not produce.

## The HTTP project

Agents write an HTTP/1.1 server library and a small example web app with it,
in Slug, against `slug.net.tcp`.

- **Grading.** A Python harness starts the agent's example app and sends raw
  requests over sockets. Its hidden cases cover:
  - request-line and header parsing, including folding and case;
  - `Content-Length` and chunked request bodies;
  - keep-alive and pipelined requests;
  - `400`, `404`, `405`, `413` and `431` responses;
  - routing with path parameters and query strings;
  - concurrent clients;
  - a slow client sending one byte at a time, and idle-connection timeouts.
- **What agents deliver.** A library with routes, handlers, request and
  response values, and a response builder, plus the example app, so the run
  also shows whether the library is pleasant to build apps with.
- **Python baseline.** The same task, using only the `socket` module and the
  rest of the standard library, but not `http.server`, `socketserver` or
  `wsgiref`, so both sides write the same layers.
- **References.** A Python reference first, to validate the hidden suite.
  The Slug reference comes from the best run, as it did for JSON.

## Decisions

- Windows is left out of version 1.
- The write timeout defaults to 5 seconds.
- Agents in the HTTP project get `slug.net.tcp` only, with no stub HTTP
  module.
