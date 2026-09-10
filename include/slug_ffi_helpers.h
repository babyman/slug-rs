#ifndef SLUG_FFI_HELPERS_H
#define SLUG_FFI_HELPERS_H

/*
 * Convenience helpers for the private slug_ffi_prototype ABI.
 *
 * This header is not an ABI surface. It owns the recurring C lifecycle work
 * for a durable native event source: a callback-thread-only receiver handle,
 * a worker-thread producer, bounded backpressure, and deferred teardown.
 */

#include "slug_ffi_prototype.h"

#include <stdlib.h>

#if defined(_WIN32)
#include <windows.h>
typedef CRITICAL_SECTION slug_ffi_helpers_mutex;
static void slug_ffi_helpers_mutex_init(slug_ffi_helpers_mutex *mutex) {
  InitializeCriticalSection(mutex);
}
static void slug_ffi_helpers_mutex_destroy(slug_ffi_helpers_mutex *mutex) {
  DeleteCriticalSection(mutex);
}
static void slug_ffi_helpers_mutex_lock(slug_ffi_helpers_mutex *mutex) {
  EnterCriticalSection(mutex);
}
static void slug_ffi_helpers_mutex_unlock(slug_ffi_helpers_mutex *mutex) {
  LeaveCriticalSection(mutex);
}
#else
#define _POSIX_C_SOURCE 200809L
#include <pthread.h>
#include <time.h>
typedef pthread_mutex_t slug_ffi_helpers_mutex;
static void slug_ffi_helpers_mutex_init(slug_ffi_helpers_mutex *mutex) {
  pthread_mutex_init(mutex, NULL);
}
static void slug_ffi_helpers_mutex_destroy(slug_ffi_helpers_mutex *mutex) {
  pthread_mutex_destroy(mutex);
}
static void slug_ffi_helpers_mutex_lock(slug_ffi_helpers_mutex *mutex) {
  pthread_mutex_lock(mutex);
}
static void slug_ffi_helpers_mutex_unlock(slug_ffi_helpers_mutex *mutex) {
  pthread_mutex_unlock(mutex);
}
#endif

typedef struct slug_ffi_async_stream slug_ffi_async_stream;

typedef struct {
  const slug_ffi_host_api *host;
  slug_ffi_producer *producer;
} slug_ffi_async_sender;

typedef void (*slug_ffi_async_stream_worker)(slug_ffi_async_sender *, void *);
typedef void (*slug_ffi_async_context_destroy)(void *);

struct slug_ffi_async_stream {
  const slug_ffi_host_api *host;
  slug_ffi_helpers_mutex mutex;
  slug_ffi_channel *channel;
  slug_ffi_producer *producer;
  uint64_t capacity;
  slug_ffi_async_stream_worker worker;
  void *context;
  slug_ffi_async_context_destroy destroy_context;
  int worker_done;
  int shutdown_requested;
};

static void slug_ffi_async_wait_for_capacity(void) {
#if defined(_WIN32)
  Sleep(1);
#else
  const struct timespec delay = {0, 1000000};
  nanosleep(&delay, NULL);
#endif
}

static void slug_ffi_async_stream_free(slug_ffi_async_stream *stream) {
  if (stream->destroy_context != NULL) stream->destroy_context(stream->context);
  slug_ffi_helpers_mutex_destroy(&stream->mutex);
  free(stream);
}

static slug_ffi_async_stream *slug_ffi_async_stream_create(
    const slug_ffi_host_api *host, uint64_t capacity,
    slug_ffi_async_stream_worker worker, void *context,
    slug_ffi_async_context_destroy destroy_context) {
  slug_ffi_async_stream *stream;
  if (host == NULL || worker == NULL) return NULL;
  stream = calloc(1, sizeof(slug_ffi_async_stream));
  if (stream == NULL) return NULL;
  stream->host = host;
  stream->capacity = capacity;
  stream->worker = worker;
  stream->context = context;
  stream->destroy_context = destroy_context;
  stream->worker_done = 1;
  slug_ffi_helpers_mutex_init(&stream->mutex);
  return stream;
}

/*
 * Sends an owned UTF-8 buffer, waiting while the bounded channel is full.
 * The buffer is released by `destroy` exactly once: by the runtime after a
 * successful send, or by this helper after cancellation or invalid input.
 */
static int slug_ffi_async_sender_send_text(slug_ffi_async_sender *sender,
                                           slug_ffi_text text,
                                           slug_ffi_producer_text_destroy_fn destroy) {
  int32_t status;
  if (sender == NULL || sender->host == NULL || sender->producer == NULL || destroy == NULL) {
    if (destroy != NULL) destroy((void *)text.data);
    return 0;
  }
  for (;;) {
    status = sender->host->producer_send_text(sender->producer, text, destroy);
    if (status == SLUG_FFI_PRODUCER_SENT) return 1;
    if (status != SLUG_FFI_PRODUCER_FULL) {
      destroy((void *)text.data);
      return 0;
    }
    slug_ffi_async_wait_for_capacity();
  }
}

/* Sends nil with the same cancellable bounded-backpressure policy. */
static int slug_ffi_async_sender_send_nil(slug_ffi_async_sender *sender) {
  int32_t status;
  if (sender == NULL || sender->host == NULL || sender->producer == NULL) return 0;
  for (;;) {
    status = sender->host->producer_send_nil(sender->producer);
    if (status == SLUG_FFI_PRODUCER_SENT) return 1;
    if (status != SLUG_FFI_PRODUCER_FULL) return 0;
    slug_ffi_async_wait_for_capacity();
  }
}

static void slug_ffi_async_stream_worker_finished(slug_ffi_async_stream *stream) {
  slug_ffi_producer *producer = NULL;
  int release_stream;
  slug_ffi_helpers_mutex_lock(&stream->mutex);
  stream->worker_done = 1;
  release_stream = stream->shutdown_requested;
  if (release_stream) {
    producer = stream->producer;
    stream->producer = NULL;
  }
  slug_ffi_helpers_mutex_unlock(&stream->mutex);
  if (producer != NULL) stream->host->producer_destroy(producer);
  if (release_stream) {
    slug_ffi_async_stream_free(stream);
  }
}

#if defined(_WIN32)
static DWORD WINAPI slug_ffi_async_stream_thread(void *raw_stream) {
  slug_ffi_async_stream *stream = raw_stream;
  slug_ffi_async_sender sender = {stream->host, stream->producer};
  stream->worker(&sender, stream->context);
  stream->host->producer_close(stream->producer);
  slug_ffi_async_stream_worker_finished(stream);
  return 0;
}
#else
static void *slug_ffi_async_stream_thread(void *raw_stream) {
  slug_ffi_async_stream *stream = raw_stream;
  slug_ffi_async_sender sender = {stream->host, stream->producer};
  stream->worker(&sender, stream->context);
  stream->host->producer_close(stream->producer);
  slug_ffi_async_stream_worker_finished(stream);
  return NULL;
}
#endif

static int slug_ffi_async_stream_start(slug_ffi_async_stream *stream) {
#if defined(_WIN32)
  HANDLE thread = CreateThread(NULL, 0, slug_ffi_async_stream_thread, stream, 0, NULL);
  if (thread == NULL) return 0;
  CloseHandle(thread);
  return 1;
#else
  pthread_t thread;
  if (pthread_create(&thread, NULL, slug_ffi_async_stream_thread, stream) != 0) return 0;
  pthread_detach(thread);
  return 1;
#endif
}

/*
 * Returns the durable receiver, creating and starting the worker on its first
 * call. The mutex stays held while cloning the receiver into the call result,
 * so stream teardown cannot invalidate the opaque channel handle mid-call.
 */
static int slug_ffi_async_stream_set_result(slug_ffi_async_stream *stream,
                                            slug_ffi_call *call) {
  slug_ffi_channel *channel;
  slug_ffi_producer *producer = NULL;
  int ok;
  if (stream == NULL) return 0;

  slug_ffi_helpers_mutex_lock(&stream->mutex);
  if (stream->channel != NULL) {
    ok = stream->host->set_channel_clone(call, stream->channel);
    slug_ffi_helpers_mutex_unlock(&stream->mutex);
    return ok;
  }

  channel = stream->host->channel_create(call, stream->capacity, &producer);
  if (channel == NULL || producer == NULL) {
    slug_ffi_helpers_mutex_unlock(&stream->mutex);
    if (channel != NULL) stream->host->channel_destroy(channel);
    if (producer != NULL) stream->host->producer_destroy(producer);
    return 0;
  }
  stream->channel = channel;
  stream->producer = producer;
  stream->worker_done = 0;
  if (!slug_ffi_async_stream_start(stream)) {
    stream->channel = NULL;
    stream->producer = NULL;
    stream->worker_done = 1;
    slug_ffi_helpers_mutex_unlock(&stream->mutex);
    stream->host->producer_destroy(producer);
    stream->host->channel_destroy(channel);
    return 0;
  }
  ok = stream->host->set_channel_clone(call, stream->channel);
  slug_ffi_helpers_mutex_unlock(&stream->mutex);
  return ok;
}

/*
 * Requests cancellation and releases the receiver. If the worker is blocked
 * in native I/O, it retains the stream and library lease until it returns.
 */
static void slug_ffi_async_stream_destroy(slug_ffi_async_stream *stream) {
  slug_ffi_channel *channel;
  slug_ffi_producer *producer = NULL;
  int worker_done;
  int release_stream;
  if (stream == NULL) return;

  slug_ffi_helpers_mutex_lock(&stream->mutex);
  stream->shutdown_requested = 1;
  channel = stream->channel;
  stream->channel = NULL;
  if (stream->producer != NULL) stream->host->producer_close(stream->producer);
  worker_done = stream->worker_done;
  if (worker_done) {
    producer = stream->producer;
    stream->producer = NULL;
  }
  release_stream = worker_done;
  slug_ffi_helpers_mutex_unlock(&stream->mutex);

  if (channel != NULL) stream->host->channel_destroy(channel);
  if (producer != NULL) stream->host->producer_destroy(producer);
  if (release_stream) {
    slug_ffi_async_stream_free(stream);
  }
}

#endif
