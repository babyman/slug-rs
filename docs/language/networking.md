# TCP networking

The `slug.net.tcp` module provides TCP servers and clients over IPv4 and IPv6.
For HTTP, the `slug.net.http` module serves HTTP/1.1 over it, with a router;
its source documents its API.
A server accepts connections and serves each in its own task:

```slug
val tcp = import("slug.net.tcp")

val listener = tcp.listen("127.0.0.1", 8080)
defer tcp.close(listener)

// Echoes what one connection sends until the peer stops sending.
val serve = fn(connection) {
  defer tcp.close(connection)
  match tcp.read(connection) {
    nil => nil
    data => {
      tcp.write(connection, data)
      recur(connection)
    }
  }
}

val loop = fn() {
  match tcp.accept(listener) {
    nil => nil
    connection => {
      spawn { serve(connection) }
      recur()
    }
  }
}
loop()
```

A client connects, writes, and reads:

```slug
val tcp = import("slug.net.tcp")
val {fromBytes} = import("slug.string")

val connection = tcp.connect("127.0.0.1", 8080)
defer tcp.close(connection)
tcp.write(connection, "hello")
tcp.shutdownWrite(connection)
println(fromBytes(tcp.read(connection) ?? 0x""))
```

Waiting, in `accept`, `connect`, `read`, a `write` that has to wait, and
`close`, parks only the calling task. Other tasks run meanwhile, so one slow
peer does not hold up the rest. Serving and connecting in one program needs
`spawn`.

## Types

`listen` returns a `Listener` and `accept` and `connect` return a
`Connection`. Both are struct types whose schemas the module keeps private:
annotate with `tcp.Listener` and `tcp.Connection`, but only the module creates
them. `close` closes either.

## Listening and accepting

`listen(host, port, backlog = 128)` listens on `host`, an address such as
`"127.0.0.1"` or `"::1"`, or a name such as `"localhost"`; an empty `host`
listens on every interface. A `port` of 0 picks a free port, which
`localPort(listener)` reports. `backlog` bounds the connections the system
queues before `accept` takes them.

`accept(listener, timeout = nil)` returns the next connection, or `nil` once
the listener is closed, including while a task is waiting in `accept`. With a
`timeout` in milliseconds, it throws a `net.timeout` error when no connection
arrives in time. Any number of tasks may accept from one listener.

## Connecting

`connect(host, port, timeout = 10000)` resolves `host`, connects, and returns
the connection, throwing `net.timeout` when it is not connected within
`timeout` milliseconds and `net.refused` when nothing listens there.

## Reading

`read(connection, timeout = nil):bytes|nil` returns the next bytes the
connection receives, or `nil` at end of stream, once the peer has closed or
shut down its sending side. A read holds at most 16 KiB. Its boundaries are
wherever the bytes happened to arrive, not where the peer's writes were, so a
protocol reads until it has what it needs. With a `timeout` in milliseconds,
`read` throws `net.timeout` when nothing arrives in time.

`chunks(connection)` is the channel `read` takes from, for a `select` that
also waits on other channels or timers. It receives bytes and closes at end of
stream; a failed receive sends a number, the system's error code, before it
closes.

Up to 16 reads' worth of received bytes wait for `read`. When that many are
unread, the connection stops receiving, and the system slows the peer's
sending, so a slow reader never buffers without bound.

`slug.string.fromBytes(data)` converts received UTF-8 bytes to a string, and
throws `string.utf8` for bytes that are not UTF-8. Bytes support indexing,
slicing, and `+`.

## Writing

`write(connection, data)` sends all of `data`, bytes or a string as UTF-8. It
queues the data and returns; the connection sends it as the peer takes it.
While more than 256 KiB is queued, `write` waits, and it throws `net.timeout`
when the peer takes nothing for the write timeout: 5 seconds, or what
`setWriteTimeout(connection, milliseconds)` set, where 0 waits indefinitely.
After a timeout the connection should be closed. A failed send, such as to a
peer that has gone, is reported by the next `write`.

`shutdownWrite(connection)` ends the sending side once what is queued has been
sent: the peer reads end of stream, `read` still returns what the peer sends,
and a later `write` fails.

## Closing

`close(connection)` first lets the peer take what writes queued, waiting at
most the write timeout for it to take each part, and then closes the
connection, ending any `read` waiting on it with `nil`. `close(listener)`
stops accepting. Closing twice does nothing; any other use of a closed
listener or connection throws a `native.resource_closed` error. Close
listeners and connections explicitly, normally with `defer`; a program that
exits with them open does not wait for them.

## Errors

Failures throw `Error` values whose `type` is:

| Type | Failure |
|---|---|
| `net.address` | A host that does not resolve, or a port that is not an integer from 0 to 65535. |
| `net.refused` | Nothing listens at the address. |
| `net.timeout` | A timeout elapsed. |
| `net.error` | Any other failure, such as a peer that reset the connection, with the system's message. |

## Availability

The module is a native clutch for macOS and Linux. It is not available on
Windows or embedded hosts. In a build without Slug-managed concurrency, there
is no `spawn`, and `accept` and `read` with a `timeout` fail because `select`
has no timers; everything else works.
