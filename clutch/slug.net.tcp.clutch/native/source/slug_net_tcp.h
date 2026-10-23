#ifndef SLUG_NET_TCP_H
#define SLUG_NET_TCP_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "slug_producer.h"

/* Statuses. With SLUG_TCP_ERROR, a `detail` out parameter holds the errno;
 * with SLUG_TCP_ADDRESS, the getaddrinfo error. */
#define SLUG_TCP_OK 0
#define SLUG_TCP_ADDRESS 1
#define SLUG_TCP_REFUSED 2
#define SLUG_TCP_TIMEOUT 3
#define SLUG_TCP_ERROR 4
#define SLUG_TCP_CLOSED 5

typedef struct slug_tcp_listener slug_tcp_listener;
typedef struct slug_tcp_connecting slug_tcp_connecting;
typedef struct slug_tcp_connection slug_tcp_connection;

/* Listens on `host` and `port` and starts an accept thread. For each accepted
 * connection the thread queues the socket and sends 0 through `ready`; after a
 * fatal accept error it sends -errno; when it stops it closes `ready`. Takes
 * ownership of `ready` whatever the result. Writes the listener and returns
 * SLUG_TCP_OK, or returns another status with `detail`. */
int32_t slug_tcp_listen(const char *host, int64_t port, int64_t backlog, slug_producer *ready,
                        slug_tcp_listener **listener, int64_t *detail);

/* The port `listener` is bound to. */
int64_t slug_tcp_local_port(slug_tcp_listener *listener);

/* Moves the oldest queued connection out of `listener` and returns
 * SLUG_TCP_OK, or returns SLUG_TCP_CLOSED when none is queued. */
int32_t slug_tcp_adopt(slug_tcp_listener *listener, slug_tcp_connection **connection);

/* Stops the accept thread, closes every queued connection and the listening
 * socket, and frees `listener`. */
void slug_tcp_listener_release(slug_tcp_listener *listener);

/* Starts a thread that connects to `host` and `port` within `timeout_ms`, then
 * sends its status through `done` and closes it. Takes ownership of `done`
 * whatever the result. Writes the pending connection and returns SLUG_TCP_OK,
 * or returns SLUG_TCP_ERROR with `detail`. */
int32_t slug_tcp_connect(const char *host, int64_t port, int64_t timeout_ms, slug_producer *done,
                         slug_tcp_connecting **connecting, int64_t *detail);

/* After `done` has received the status, returns it: with SLUG_TCP_OK, writes
 * the connection, which is taken once; otherwise writes `detail`. */
int32_t slug_tcp_finish_connect(slug_tcp_connecting *connecting,
                                slug_tcp_connection **connection, int64_t *detail);

/* Releases `connecting`. A connection it still holds is closed once the
 * connect thread has finished. */
void slug_tcp_connecting_release(slug_tcp_connecting *connecting);

/* Closes the connection: the writer sends what is queued, waiting at most
 * the write timeout for the peer to take each part, then shuts down the
 * socket, which wakes the reader. Releases the resource's reference; the
 * socket closes with the last reference. */
void slug_tcp_connection_release(slug_tcp_connection *connection);

/* Starts the connection's reader and writer threads. The reader sends what
 * the connection receives through `chunks` as bytes, at most 16 KiB at a
 * time, and closes `chunks` at end of stream; a failed receive first sends
 * its errno as a number. The writer sends what writes queue; when a write is
 * waiting it sends a tick through `writable` as the queue shrinks, and it
 * closes `writable` when it stops. Takes ownership of both producers whatever
 * the result. A second call does nothing. */
int32_t slug_tcp_start(slug_tcp_connection *connection, slug_producer *chunks,
                       slug_producer *writable, int64_t *detail);

/* Queues a copy of `data` for the writer, never blocking, and writes the bytes
 * now queued. After a failed send, returns its status and errno: a send that
 * timed out once the connection was closing is SLUG_TCP_TIMEOUT. */
int32_t slug_tcp_write(slug_tcp_connection *connection, const uint8_t *data, size_t length,
                       int64_t *queued, int64_t *detail);

/* As slug_tcp_write, for text. */
int32_t slug_tcp_write_text(slug_tcp_connection *connection, const char *text, size_t length,
                            int64_t *queued, int64_t *detail);

/* Writes the bytes queued and asks the writer for a tick through `writable`:
 * 0 when the queue next shrinks or writing fails, or 1 when the peer takes
 * nothing for the write timeout. Returns the status writing failed with, if
 * it has. */
int32_t slug_tcp_wait_writable(slug_tcp_connection *connection, int64_t *queued,
                               int64_t *detail);

/* Sets how long the peer may take nothing before a send times out; 0 waits
 * indefinitely. */
void slug_tcp_set_write_timeout(slug_tcp_connection *connection, int64_t milliseconds);

/* Shuts down the sending side once what is queued has been sent, so the peer
 * reads end of stream. Later writes fail with EPIPE. */
int32_t slug_tcp_shutdown_write(slug_tcp_connection *connection, int64_t *detail);

/* The peer's address and port, as `host:port` or `[host]:port`. */
const char *slug_tcp_peer(slug_tcp_connection *connection);

/* A message describing `status` and its `detail`. */
const char *slug_tcp_message(int32_t status, int64_t detail);

#endif
