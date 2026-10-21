/* POSIX sockets; shims build with -std=c11. */
#define _POSIX_C_SOURCE 200809L
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif

#include "slug_net_tcp.h"

#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <sys/time.h>
#include <poll.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

/* Bytes a reader receives at once, and so the largest chunk it sends. */
#define READ_BUFFER 16384
/* How long the peer may take nothing before a write times out. */
#define DEFAULT_WRITE_TIMEOUT_MS 5000
/* Bytes the writer sends at once. */
#define WRITE_SLICE 65536

/* Bytes waiting for the writer, oldest first. */
struct pending {
  struct pending *next;
  size_t length;
  size_t sent;
  uint8_t data[];
};

struct slug_tcp_connection {
  int fd;
  /* The resource and the reader and writer threads each hold one reference;
   * the socket closes when the last goes. */
  atomic_int references;
  atomic_bool closed;
  atomic_bool started;
  char peer[64];
  /* The writer's queue, guarded by `lock`. */
  pthread_mutex_t lock;
  pthread_cond_t more;
  struct pending *first;
  struct pending *last;
  size_t queued;
  /* The errno a failed send ended writing with, or 0. */
  int write_error;
  /* Set by `slug_tcp_wait_writable`: the writer sends a tick through
   * `writable` when the queue next shrinks. */
  bool waiting;
  bool writing;
  /* `slug_tcp_shutdown_write` was called; `shut_down` once the writer has. */
  bool shutdown_requested;
  bool shut_down;
  slug_producer *writable;
};

struct slug_tcp_listener {
  int fd;
  /* Writing to wake[1] stops the accept thread's poll. */
  int wake[2];
  int64_t port;
  pthread_t thread;
  slug_producer *ready;
  atomic_bool closing;
  pthread_mutex_t lock;
  /* Accepted sockets not yet adopted, oldest first. */
  int *queue;
  size_t queued;
  size_t capacity;
};

struct slug_tcp_connecting {
  char *host;
  char port[8];
  int64_t timeout_ms;
  slug_producer *done;
  pthread_mutex_t lock;
  /* The connect thread and the resource each hold one reference. */
  atomic_int references;
  int32_t status;
  int64_t detail;
  int fd;
};

static void pause_briefly(void) {
  struct timespec delay = {0, 1000000};
  nanosleep(&delay, NULL);
}

static void set_close_on_exec(int fd) { fcntl(fd, F_SETFD, fcntl(fd, F_GETFD) | FD_CLOEXEC); }

static void set_blocking(int fd, bool blocking) {
  int flags = fcntl(fd, F_GETFL);
  fcntl(fd, F_SETFL, blocking ? flags & ~O_NONBLOCK : flags | O_NONBLOCK);
}

/* A connected socket: blocking, closed on exec, and never raising SIGPIPE
 * where the platform has a socket option for it. */
static void prepare_connected(int fd) {
  set_blocking(fd, true);
  set_close_on_exec(fd);
#if defined(SO_NOSIGPIPE)
  int on = 1;
  setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, sizeof on);
#endif
}

static void set_write_timeout(int fd, int64_t milliseconds) {
  struct timeval timeout = {(time_t)(milliseconds / 1000), (suseconds_t)(milliseconds % 1000) * 1000};
  setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof timeout);
}

/* The peer's address as `host:port`, or `[host]:port` for IPv6. */
static void describe_peer(int fd, char *buffer, size_t size) {
  struct sockaddr_storage address;
  socklen_t length = sizeof address;
  char host[INET6_ADDRSTRLEN + 8];
  char port[8];
  buffer[0] = '\0';
  if (getpeername(fd, (struct sockaddr *)&address, &length) != 0 ||
      getnameinfo((struct sockaddr *)&address, length, host, sizeof host, port, sizeof port,
                  NI_NUMERICHOST | NI_NUMERICSERV) != 0) {
    return;
  }
  snprintf(buffer, size, address.ss_family == AF_INET6 ? "[%s]:%s" : "%s:%s", host, port);
}

static slug_tcp_connection *new_connection(int fd) {
  slug_tcp_connection *connection = malloc(sizeof *connection);
  if (connection == NULL) {
    close(fd);
    return NULL;
  }
  connection->fd = fd;
  atomic_init(&connection->references, 1);
  atomic_init(&connection->closed, false);
  atomic_init(&connection->started, false);
  pthread_mutex_init(&connection->lock, NULL);
  pthread_cond_init(&connection->more, NULL);
  connection->first = NULL;
  connection->last = NULL;
  connection->queued = 0;
  connection->write_error = 0;
  connection->waiting = false;
  connection->writing = false;
  connection->shutdown_requested = false;
  connection->shut_down = false;
  connection->writable = NULL;
  set_write_timeout(fd, DEFAULT_WRITE_TIMEOUT_MS);
  describe_peer(fd, connection->peer, sizeof connection->peer);
  return connection;
}

static bool format_port(int64_t port, char *buffer, size_t size) {
  if (port < 0 || port > 65535) return false;
  snprintf(buffer, size, "%d", (int)port);
  return true;
}

/* Sends `value`, retrying while the channel is full. Returns false when the
 * receiver has gone or `stop` is set. */
static bool send_value(slug_producer *producer, int64_t value, atomic_bool *stop) {
  for (;;) {
    switch (slug_producer_send_i64(producer, value)) {
      case SLUG_PRODUCER_SENT:
        return true;
      case SLUG_PRODUCER_FULL:
        if (stop != NULL && atomic_load(stop)) return false;
        pause_briefly();
        break;
      default:
        return false;
    }
  }
}

static bool enqueue(slug_tcp_listener *listener, int fd) {
  pthread_mutex_lock(&listener->lock);
  if (atomic_load(&listener->closing)) {
    pthread_mutex_unlock(&listener->lock);
    return false;
  }
  if (listener->queued == listener->capacity) {
    size_t capacity = listener->capacity == 0 ? 16 : listener->capacity * 2;
    int *queue = realloc(listener->queue, capacity * sizeof *queue);
    if (queue == NULL) {
      pthread_mutex_unlock(&listener->lock);
      return false;
    }
    listener->queue = queue;
    listener->capacity = capacity;
  }
  listener->queue[listener->queued++] = fd;
  pthread_mutex_unlock(&listener->lock);
  return true;
}

static void *accept_loop(void *argument) {
  slug_tcp_listener *listener = argument;
  struct pollfd fds[2] = {{listener->fd, POLLIN, 0}, {listener->wake[0], POLLIN, 0}};
  while (!atomic_load(&listener->closing)) {
    if (poll(fds, 2, -1) < 0) {
      if (errno == EINTR) continue;
      send_value(listener->ready, -errno, &listener->closing);
      break;
    }
    if (fds[1].revents != 0) break;
    if (fds[0].revents == 0) continue;
    int fd = accept(listener->fd, NULL, NULL);
    if (fd < 0) {
      if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR || errno == ECONNABORTED ||
          errno == EPROTO) {
        continue;
      }
      if (errno == EMFILE || errno == ENFILE || errno == ENOBUFS || errno == ENOMEM) {
        /* Out of descriptors or memory: wait for some to be released. */
        pause_briefly();
        continue;
      }
      send_value(listener->ready, -errno, &listener->closing);
      break;
    }
    prepare_connected(fd);
    if (!enqueue(listener, fd)) {
      close(fd);
      break;
    }
    if (!send_value(listener->ready, 0, &listener->closing)) break;
  }
  slug_producer_close(listener->ready);
  slug_producer_release(listener->ready);
  listener->ready = NULL;
  return NULL;
}

/* Resolves `host` and `port` into `addresses`, returning a status. */
static int32_t resolve(const char *host, const char *port, bool passive,
                       struct addrinfo **addresses, int64_t *detail) {
  struct addrinfo hints;
  memset(&hints, 0, sizeof hints);
  hints.ai_family = AF_UNSPEC;
  hints.ai_socktype = SOCK_STREAM;
  hints.ai_flags = AI_NUMERICSERV | (passive ? AI_PASSIVE : 0);
  int result = getaddrinfo(host[0] == '\0' ? NULL : host, port, &hints, addresses);
  if (result != 0) {
    *detail = result;
    return SLUG_TCP_ADDRESS;
  }
  return SLUG_TCP_OK;
}

int32_t slug_tcp_listen(const char *host, int64_t port, int64_t backlog, slug_producer *ready,
                        slug_tcp_listener **listener, int64_t *detail) {
  *listener = NULL;
  *detail = 0;
  char service[8];
  if (!format_port(port, service, sizeof service)) {
    slug_producer_release(ready);
    *detail = EINVAL;
    return SLUG_TCP_ERROR;
  }
  struct addrinfo *addresses;
  int32_t status = resolve(host, service, true, &addresses, detail);
  if (status != SLUG_TCP_OK) {
    slug_producer_release(ready);
    return status;
  }
  int fd = -1;
  int error = 0;
  for (struct addrinfo *address = addresses; address != NULL; address = address->ai_next) {
    fd = socket(address->ai_family, address->ai_socktype, address->ai_protocol);
    if (fd < 0) {
      error = errno;
      continue;
    }
    int on = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &on, sizeof on);
    if (bind(fd, address->ai_addr, address->ai_addrlen) == 0 &&
        listen(fd, backlog > 0 && backlog < 65536 ? (int)backlog : 128) == 0) {
      break;
    }
    error = errno;
    close(fd);
    fd = -1;
  }
  freeaddrinfo(addresses);
  if (fd < 0) {
    slug_producer_release(ready);
    *detail = error;
    return SLUG_TCP_ERROR;
  }
  set_close_on_exec(fd);
  set_blocking(fd, false);

  slug_tcp_listener *created = calloc(1, sizeof *created);
  if (created == NULL || pipe(created->wake) != 0) {
    error = created == NULL ? ENOMEM : errno;
    free(created);
    close(fd);
    slug_producer_release(ready);
    *detail = error;
    return SLUG_TCP_ERROR;
  }
  set_close_on_exec(created->wake[0]);
  set_close_on_exec(created->wake[1]);
  created->fd = fd;
  created->ready = ready;
  atomic_init(&created->closing, false);
  pthread_mutex_init(&created->lock, NULL);
  struct sockaddr_storage bound;
  socklen_t length = sizeof bound;
  if (getsockname(fd, (struct sockaddr *)&bound, &length) == 0) {
    created->port = bound.ss_family == AF_INET6
                        ? ntohs(((struct sockaddr_in6 *)&bound)->sin6_port)
                        : ntohs(((struct sockaddr_in *)&bound)->sin_port);
  }
  if ((error = pthread_create(&created->thread, NULL, accept_loop, created)) != 0) {
    close(created->wake[0]);
    close(created->wake[1]);
    pthread_mutex_destroy(&created->lock);
    free(created);
    close(fd);
    slug_producer_release(ready);
    *detail = error;
    return SLUG_TCP_ERROR;
  }
  *listener = created;
  return SLUG_TCP_OK;
}

int64_t slug_tcp_local_port(slug_tcp_listener *listener) { return listener->port; }

int32_t slug_tcp_adopt(slug_tcp_listener *listener, slug_tcp_connection **connection) {
  *connection = NULL;
  pthread_mutex_lock(&listener->lock);
  if (listener->queued == 0) {
    pthread_mutex_unlock(&listener->lock);
    return SLUG_TCP_CLOSED;
  }
  int fd = listener->queue[0];
  listener->queued--;
  memmove(listener->queue, listener->queue + 1, listener->queued * sizeof *listener->queue);
  pthread_mutex_unlock(&listener->lock);
  *connection = new_connection(fd);
  return *connection == NULL ? SLUG_TCP_CLOSED : SLUG_TCP_OK;
}

void slug_tcp_listener_release(slug_tcp_listener *listener) {
  atomic_store(&listener->closing, true);
  char wake = 0;
  while (write(listener->wake[1], &wake, 1) < 0 && errno == EINTR) {
  }
  pthread_join(listener->thread, NULL);
  for (size_t i = 0; i < listener->queued; i++) close(listener->queue[i]);
  free(listener->queue);
  close(listener->fd);
  close(listener->wake[0]);
  close(listener->wake[1]);
  pthread_mutex_destroy(&listener->lock);
  free(listener);
}

static void drop_connecting(slug_tcp_connecting *connecting) {
  if (atomic_fetch_sub(&connecting->references, 1) != 1) return;
  if (connecting->fd >= 0) close(connecting->fd);
  pthread_mutex_destroy(&connecting->lock);
  free(connecting->host);
  free(connecting);
}

static int64_t now_ms(void) {
  struct timespec now;
  clock_gettime(CLOCK_MONOTONIC, &now);
  return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

/* Connects to one address within `deadline`, returning the socket or -1 with
 * `status` and `error` set. */
static int connect_one(const struct addrinfo *address, int64_t deadline, int32_t *status,
                       int *error) {
  int fd = socket(address->ai_family, address->ai_socktype, address->ai_protocol);
  if (fd < 0) {
    *status = SLUG_TCP_ERROR;
    *error = errno;
    return -1;
  }
  set_blocking(fd, false);
  if (connect(fd, address->ai_addr, address->ai_addrlen) != 0) {
    if (errno != EINPROGRESS) {
      *error = errno;
      *status = errno == ECONNREFUSED ? SLUG_TCP_REFUSED : SLUG_TCP_ERROR;
      close(fd);
      return -1;
    }
    for (;;) {
      int64_t remaining = deadline - now_ms();
      if (remaining <= 0) {
        *status = SLUG_TCP_TIMEOUT;
        *error = ETIMEDOUT;
        close(fd);
        return -1;
      }
      struct pollfd ready = {fd, POLLOUT, 0};
      int polled = poll(&ready, 1, remaining > 2147483647 ? 2147483647 : (int)remaining);
      if (polled < 0 && errno == EINTR) continue;
      if (polled <= 0) continue;
      break;
    }
    int failure = 0;
    socklen_t length = sizeof failure;
    getsockopt(fd, SOL_SOCKET, SO_ERROR, &failure, &length);
    if (failure != 0) {
      *error = failure;
      *status = failure == ECONNREFUSED ? SLUG_TCP_REFUSED : SLUG_TCP_ERROR;
      close(fd);
      return -1;
    }
  }
  prepare_connected(fd);
  return fd;
}

static void *connect_thread(void *argument) {
  slug_tcp_connecting *connecting = argument;
  int64_t deadline = now_ms() + connecting->timeout_ms;
  struct addrinfo *addresses;
  int64_t detail = 0;
  int32_t status = resolve(connecting->host, connecting->port, false, &addresses, &detail);
  int fd = -1;
  if (status == SLUG_TCP_OK) {
    int error = 0;
    status = SLUG_TCP_ERROR;
    for (struct addrinfo *address = addresses; address != NULL && fd < 0;
         address = address->ai_next) {
      fd = connect_one(address, deadline, &status, &error);
      if (status == SLUG_TCP_TIMEOUT) break;
    }
    freeaddrinfo(addresses);
    if (fd >= 0) {
      status = SLUG_TCP_OK;
    } else {
      detail = error;
    }
  }
  pthread_mutex_lock(&connecting->lock);
  connecting->status = status;
  connecting->detail = detail;
  connecting->fd = fd;
  pthread_mutex_unlock(&connecting->lock);
  send_value(connecting->done, status, NULL);
  slug_producer_close(connecting->done);
  slug_producer_release(connecting->done);
  drop_connecting(connecting);
  return NULL;
}

int32_t slug_tcp_connect(const char *host, int64_t port, int64_t timeout_ms, slug_producer *done,
                         slug_tcp_connecting **connecting, int64_t *detail) {
  *connecting = NULL;
  *detail = 0;
  slug_tcp_connecting *created = calloc(1, sizeof *created);
  if (created == NULL || (created->host = strdup(host)) == NULL) {
    free(created);
    slug_producer_release(done);
    *detail = ENOMEM;
    return SLUG_TCP_ERROR;
  }
  if (!format_port(port, created->port, sizeof created->port)) {
    free(created->host);
    free(created);
    slug_producer_release(done);
    *detail = EINVAL;
    return SLUG_TCP_ERROR;
  }
  created->timeout_ms = timeout_ms < 0 ? 0 : timeout_ms;
  created->done = done;
  created->status = SLUG_TCP_CLOSED;
  created->fd = -1;
  atomic_init(&created->references, 2);
  pthread_mutex_init(&created->lock, NULL);
  pthread_t thread;
  int error = pthread_create(&thread, NULL, connect_thread, created);
  if (error != 0) {
    pthread_mutex_destroy(&created->lock);
    free(created->host);
    free(created);
    slug_producer_release(done);
    *detail = error;
    return SLUG_TCP_ERROR;
  }
  pthread_detach(thread);
  *connecting = created;
  return SLUG_TCP_OK;
}

int32_t slug_tcp_finish_connect(slug_tcp_connecting *connecting,
                                slug_tcp_connection **connection, int64_t *detail) {
  *connection = NULL;
  pthread_mutex_lock(&connecting->lock);
  int32_t status = connecting->status;
  *detail = connecting->detail;
  int fd = connecting->fd;
  if (status == SLUG_TCP_OK) {
    connecting->fd = -1;
    /* Taken once: a second call finds no connection. */
    connecting->status = SLUG_TCP_CLOSED;
  }
  pthread_mutex_unlock(&connecting->lock);
  if (status == SLUG_TCP_OK && (*connection = new_connection(fd)) == NULL) {
    *detail = ENOMEM;
    return SLUG_TCP_ERROR;
  }
  return status;
}

void slug_tcp_connecting_release(slug_tcp_connecting *connecting) { drop_connecting(connecting); }

static void free_pending(struct pending *pending) {
  while (pending != NULL) {
    struct pending *next = pending->next;
    free(pending);
    pending = next;
  }
}

static void drop_connection(slug_tcp_connection *connection) {
  if (atomic_fetch_sub(&connection->references, 1) != 1) return;
  free_pending(connection->first);
  pthread_cond_destroy(&connection->more);
  pthread_mutex_destroy(&connection->lock);
  close(connection->fd);
  free(connection);
}

void slug_tcp_connection_release(slug_tcp_connection *connection) {
  atomic_store(&connection->closed, true);
  pthread_mutex_lock(&connection->lock);
  bool writing = connection->writing;
  pthread_cond_signal(&connection->more);
  pthread_mutex_unlock(&connection->lock);
  /* Without a writer to finish the queue, shut down now, waking the reader
   * blocked in recv. The writer shuts down once it has finished. */
  if (!writing) shutdown(connection->fd, SHUT_RDWR);
  drop_connection(connection);
}

/* Ticks through `writable`. */
#define TICK_PROGRESS 0
#define TICK_STALLED 1

/* With the lock held: tells a waiting `write` that the queue shrank, that
 * writing failed, or with TICK_STALLED that the peer took nothing for the
 * write timeout. A tick already in the channel will do. */
static void notify_writable(slug_tcp_connection *connection, int64_t tick) {
  if (!connection->waiting) return;
  connection->waiting = false;
  slug_producer_send_i64(connection->writable, tick);
}

struct threads {
  slug_tcp_connection *connection;
  slug_producer *chunks;
};

static void *read_loop(void *argument) {
  struct threads *threads = argument;
  slug_tcp_connection *connection = threads->connection;
  slug_producer *chunks = threads->chunks;
  free(threads);
  uint8_t *buffer = malloc(READ_BUFFER);
  int error = buffer == NULL ? ENOMEM : 0;
  while (buffer != NULL) {
    ssize_t received = recv(connection->fd, buffer, READ_BUFFER, 0);
    if (received == 0) break;
    if (received < 0) {
      if (errno == EINTR) continue;
      /* A shutdown by `close` is not a failure. */
      if (!atomic_load(&connection->closed)) error = errno;
      break;
    }
    bool sent = false;
    while (!sent) {
      switch (slug_producer_send_bytes(chunks, buffer, (size_t)received)) {
        case SLUG_PRODUCER_SENT:
          sent = true;
          break;
        case SLUG_PRODUCER_FULL:
          if (atomic_load(&connection->closed)) goto done;
          pause_briefly();
          break;
        default:
          goto done;
      }
    }
  }
  if (error != 0) send_value(chunks, error, &connection->closed);
done:
  free(buffer);
  slug_producer_close(chunks);
  slug_producer_release(chunks);
  drop_connection(connection);
  return NULL;
}

/* Sends the queue's bytes until the connection is closed and the queue is
 * empty, or a send fails. A send that times out is retried, except once the
 * connection is closed: then the peer has had the write timeout to take what
 * remains. */
static void *write_loop(void *argument) {
  slug_tcp_connection *connection = argument;
  int flags = 0;
#if defined(MSG_NOSIGNAL)
  flags = MSG_NOSIGNAL;
#endif
  pthread_mutex_lock(&connection->lock);
  for (;;) {
    while (connection->first == NULL && !atomic_load(&connection->closed) &&
           connection->shutdown_requested == connection->shut_down) {
      pthread_cond_wait(&connection->more, &connection->lock);
    }
    struct pending *pending = connection->first;
    if (pending == NULL && connection->shutdown_requested && !connection->shut_down) {
      shutdown(connection->fd, SHUT_WR);
      connection->shut_down = true;
      continue;
    }
    if (pending == NULL) break;
    size_t length = pending->length - pending->sent;
    if (length > WRITE_SLICE) length = WRITE_SLICE;
    const uint8_t *data = pending->data + pending->sent;
    pthread_mutex_unlock(&connection->lock);
    ssize_t sent = send(connection->fd, data, length, flags);
    int error = sent < 0 ? errno : 0;
    pthread_mutex_lock(&connection->lock);
    if (sent < 0) {
      if (error == EINTR) continue;
      if ((error == EAGAIN || error == EWOULDBLOCK) && !atomic_load(&connection->closed)) {
        notify_writable(connection, TICK_STALLED);
        continue;
      }
      connection->write_error = error == EAGAIN || error == EWOULDBLOCK ? ETIMEDOUT : error;
      free_pending(connection->first);
      connection->first = connection->last = NULL;
      connection->queued = 0;
      notify_writable(connection, TICK_PROGRESS);
      break;
    }
    pending->sent += (size_t)sent;
    connection->queued -= (size_t)sent;
    if (pending->sent == pending->length) {
      connection->first = pending->next;
      if (connection->first == NULL) connection->last = NULL;
      free(pending);
    }
    notify_writable(connection, TICK_PROGRESS);
  }
  connection->writing = false;
  slug_producer *writable = connection->writable;
  connection->writable = NULL;
  pthread_mutex_unlock(&connection->lock);
  slug_producer_close(writable);
  slug_producer_release(writable);
  /* Once closed and finished, wake the reader and let the peer see the end. */
  if (atomic_load(&connection->closed)) shutdown(connection->fd, SHUT_RDWR);
  drop_connection(connection);
  return NULL;
}

int32_t slug_tcp_start(slug_tcp_connection *connection, slug_producer *chunks,
                       slug_producer *writable, int64_t *detail) {
  *detail = 0;
  if (atomic_exchange(&connection->started, true)) {
    slug_producer_release(chunks);
    slug_producer_release(writable);
    return SLUG_TCP_OK;
  }
  struct threads *threads = malloc(sizeof *threads);
  if (threads == NULL) {
    slug_producer_release(chunks);
    slug_producer_release(writable);
    *detail = ENOMEM;
    return SLUG_TCP_ERROR;
  }
  threads->connection = connection;
  threads->chunks = chunks;
  pthread_mutex_lock(&connection->lock);
  connection->writable = writable;
  connection->writing = true;
  pthread_mutex_unlock(&connection->lock);
  atomic_fetch_add(&connection->references, 2);
  pthread_t reader;
  pthread_t writer;
  int error = pthread_create(&writer, NULL, write_loop, connection);
  if (error != 0) {
    pthread_mutex_lock(&connection->lock);
    connection->writing = false;
    connection->writable = NULL;
    pthread_mutex_unlock(&connection->lock);
    atomic_fetch_sub(&connection->references, 2);
    free(threads);
    slug_producer_release(chunks);
    slug_producer_release(writable);
    *detail = error;
    return SLUG_TCP_ERROR;
  }
  pthread_detach(writer);
  error = pthread_create(&reader, NULL, read_loop, threads);
  if (error != 0) {
    /* The writer runs; without a reader, nothing is received. */
    atomic_fetch_sub(&connection->references, 1);
    free(threads);
    slug_producer_release(chunks);
    *detail = error;
    return SLUG_TCP_ERROR;
  }
  pthread_detach(reader);
  return SLUG_TCP_OK;
}

/* With the lock held: whether the connection can still take writes. */
static int32_t writable_status(slug_tcp_connection *connection, int64_t *detail) {
  if (connection->write_error != 0) {
    *detail = connection->write_error;
    return *detail == ETIMEDOUT ? SLUG_TCP_TIMEOUT : SLUG_TCP_ERROR;
  }
  if (connection->shutdown_requested) {
    *detail = EPIPE;
    return SLUG_TCP_ERROR;
  }
  return connection->writing ? SLUG_TCP_OK : SLUG_TCP_CLOSED;
}

/* Queues a copy of `data` for the writer, writing the bytes now queued. */
static int32_t queue_write(slug_tcp_connection *connection, const uint8_t *data, size_t length,
                       int64_t *queued, int64_t *detail) {
  *detail = 0;
  pthread_mutex_lock(&connection->lock);
  *queued = (int64_t)connection->queued;
  int32_t status = writable_status(connection, detail);
  if (status != SLUG_TCP_OK) {
    pthread_mutex_unlock(&connection->lock);
    return status;
  }
  if (length > 0) {
    struct pending *pending = malloc(sizeof *pending + length);
    if (pending == NULL) {
      pthread_mutex_unlock(&connection->lock);
      *detail = ENOMEM;
      return SLUG_TCP_ERROR;
    }
    pending->next = NULL;
    pending->length = length;
    pending->sent = 0;
    memcpy(pending->data, data, length);
    if (connection->last == NULL) {
      connection->first = pending;
    } else {
      connection->last->next = pending;
    }
    connection->last = pending;
    connection->queued += length;
    *queued = (int64_t)connection->queued;
    pthread_cond_signal(&connection->more);
  }
  pthread_mutex_unlock(&connection->lock);
  return SLUG_TCP_OK;
}

int32_t slug_tcp_write(slug_tcp_connection *connection, const uint8_t *data, size_t length,
                       int64_t *queued, int64_t *detail) {
  return queue_write(connection, data, length, queued, detail);
}

int32_t slug_tcp_write_text(slug_tcp_connection *connection, const char *text, size_t length,
                            int64_t *queued, int64_t *detail) {
  return queue_write(connection, (const uint8_t *)text, length, queued, detail);
}

int32_t slug_tcp_wait_writable(slug_tcp_connection *connection, int64_t *queued,
                               int64_t *detail) {
  *detail = 0;
  pthread_mutex_lock(&connection->lock);
  *queued = (int64_t)connection->queued;
  int32_t status = writable_status(connection, detail);
  if (status == SLUG_TCP_OK) connection->waiting = true;
  pthread_mutex_unlock(&connection->lock);
  return status;
}

void slug_tcp_set_write_timeout(slug_tcp_connection *connection, int64_t milliseconds) {
  set_write_timeout(connection->fd, milliseconds);
}

int32_t slug_tcp_shutdown_write(slug_tcp_connection *connection, int64_t *detail) {
  *detail = 0;
  pthread_mutex_lock(&connection->lock);
  int32_t status = writable_status(connection, detail);
  if (status == SLUG_TCP_OK) {
    /* The writer shuts down once it has sent what is queued. */
    connection->shutdown_requested = true;
    pthread_cond_signal(&connection->more);
  } else if (connection->shutdown_requested) {
    status = SLUG_TCP_OK;
    *detail = 0;
  }
  pthread_mutex_unlock(&connection->lock);
  return status;
}

const char *slug_tcp_peer(slug_tcp_connection *connection) { return connection->peer; }

const char *slug_tcp_message(int32_t status, int64_t detail) {
  switch (status) {
    case SLUG_TCP_OK:
      return "success";
    case SLUG_TCP_ADDRESS:
      return gai_strerror((int)detail);
    case SLUG_TCP_TIMEOUT:
      return "the operation timed out";
    case SLUG_TCP_CLOSED:
      return "the connection is closed";
    default:
      return strerror((int)detail);
  }
}
