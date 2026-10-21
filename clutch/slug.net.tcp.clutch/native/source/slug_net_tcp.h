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

/* Shuts down and closes the connection's socket and frees `connection`. */
void slug_tcp_connection_release(slug_tcp_connection *connection);

/* A message describing `status` and its `detail`. */
const char *slug_tcp_message(int32_t status, int64_t detail);

#endif
