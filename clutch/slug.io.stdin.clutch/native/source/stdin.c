#include "slug_ffi_prototype.h"

#include <stdio.h>
#include <stdlib.h>

#if defined(_WIN32)
#include <windows.h>
typedef CRITICAL_SECTION stdin_mutex;
static void mutex_init(stdin_mutex *mutex) { InitializeCriticalSection(mutex); }
static void mutex_destroy(stdin_mutex *mutex) { DeleteCriticalSection(mutex); }
static void mutex_lock(stdin_mutex *mutex) { EnterCriticalSection(mutex); }
static void mutex_unlock(stdin_mutex *mutex) { LeaveCriticalSection(mutex); }
#else
#define _POSIX_C_SOURCE 200809L
#include <pthread.h>
#include <time.h>
typedef pthread_mutex_t stdin_mutex;
static void mutex_init(stdin_mutex *mutex) { pthread_mutex_init(mutex, NULL); }
static void mutex_destroy(stdin_mutex *mutex) { pthread_mutex_destroy(mutex); }
static void mutex_lock(stdin_mutex *mutex) { pthread_mutex_lock(mutex); }
static void mutex_unlock(stdin_mutex *mutex) { pthread_mutex_unlock(mutex); }
#endif

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})
#define STDIN_CHANNEL_CAPACITY 32u

typedef struct {
  const slug_ffi_host_api *host;
  stdin_mutex mutex;
  slug_ffi_channel *channel;
  slug_ffi_producer *producer;
  int worker_done;
  int shutdown_requested;
} stdin_state;

typedef struct {
  stdin_state *state;
  const slug_ffi_host_api *host;
  slug_ffi_producer *producer;
} stdin_context;

static void destroy_text(void *text) { free(text); }

static void wait_for_capacity(void) {
#if defined(_WIN32)
  Sleep(1);
#else
  const struct timespec delay = {0, 1000000};
  nanosleep(&delay, NULL);
#endif
}

static int send_line(const slug_ffi_host_api *host, slug_ffi_producer *producer,
                     char *line, size_t length) {
  for (;;) {
    int32_t status = host->producer_send_text(
        producer, (slug_ffi_text){line, (uint64_t)length}, destroy_text);
    if (status == SLUG_FFI_PRODUCER_SENT) return 1;
    if (status != SLUG_FFI_PRODUCER_FULL) {
      free(line);
      return 0;
    }
    wait_for_capacity();
  }
}

static void read_stdin_lines(const slug_ffi_host_api *host,
                             slug_ffi_producer *producer) {
  char *line = NULL;
  size_t length = 0;
  size_t capacity = 0;
  int character;

  while ((character = fgetc(stdin)) != EOF) {
    if (character == '\n') {
      if (length > 0 && line[length - 1] == '\r') length--;
      if (line == NULL) {
        line = malloc(1);
        if (line == NULL) break;
      }
      if (!send_line(host, producer, line, length)) {
        host->producer_close(producer);
        return;
      }
      line = NULL;
      length = 0;
      capacity = 0;
      continue;
    }

    if (length == capacity) {
      size_t next_capacity = capacity == 0 ? 128 : capacity * 2;
      if (next_capacity <= capacity) {
        free(line);
        host->producer_close(producer);
        return;
      }
      char *expanded = realloc(line, next_capacity);
      if (expanded == NULL) {
        free(line);
        host->producer_close(producer);
        return;
      }
      line = expanded;
      capacity = next_capacity;
    }
    line[length++] = (char)character;
  }

  if (ferror(stdin)) {
    free(line);
    host->producer_close(producer);
    return;
  }
  if (line != NULL && !send_line(host, producer, line, length)) {
    host->producer_close(producer);
    return;
  }
  host->producer_close(producer);
}

static void worker_finished(stdin_context *context) {
  stdin_state *state = context->state;
  slug_ffi_producer *producer = NULL;

  mutex_lock(&state->mutex);
  state->worker_done = 1;
  if (state->shutdown_requested) {
    producer = state->producer;
    state->producer = NULL;
  }
  mutex_unlock(&state->mutex);

  if (producer != NULL) {
    context->host->producer_destroy(producer);
    mutex_destroy(&state->mutex);
    free(state);
  }
}

#if defined(_WIN32)
static DWORD WINAPI stdin_worker(void *raw_context) {
  stdin_context *context = raw_context;
  read_stdin_lines(context->host, context->producer);
  worker_finished(context);
  free(context);
  return 0;
}
#else
static void *stdin_worker(void *raw_context) {
  stdin_context *context = raw_context;
  read_stdin_lines(context->host, context->producer);
  worker_finished(context);
  free(context);
  return NULL;
}
#endif

static int start_worker(stdin_context *context) {
#if defined(_WIN32)
  HANDLE thread = CreateThread(NULL, 0, stdin_worker, context, 0, NULL);
  if (thread == NULL) return 0;
  CloseHandle(thread);
  return 1;
#else
  pthread_t thread;
  if (pthread_create(&thread, NULL, stdin_worker, context) != 0) return 0;
  pthread_detach(thread);
  return 1;
#endif
}

static int32_t read_lines(const slug_ffi_host_api *host, slug_ffi_call *call,
                          void *raw_state) {
  stdin_state *state = raw_state;
  slug_ffi_channel *channel;
  slug_ffi_producer *producer = NULL;
  stdin_context *context = NULL;

  if (state == NULL) {
    host->set_error(call, TEXT("native.io"), TEXT("standard-input state is unavailable"));
    return SLUG_FFI_ERROR;
  }

  mutex_lock(&state->mutex);
  if (state->channel != NULL) {
    channel = state->channel;
    mutex_unlock(&state->mutex);
    return host->set_channel_clone(call, channel) ? SLUG_FFI_OK : SLUG_FFI_ERROR;
  }

  channel = host->channel_create(call, STDIN_CHANNEL_CAPACITY, &producer);
  context = malloc(sizeof(stdin_context));
  if (channel == NULL || producer == NULL || context == NULL) {
    mutex_unlock(&state->mutex);
    if (channel != NULL) host->channel_destroy(channel);
    if (producer != NULL) host->producer_destroy(producer);
    free(context);
    host->set_error(call, TEXT("native.io"), TEXT("cannot create standard-input stream"));
    return SLUG_FFI_ERROR;
  }

  context->state = state;
  context->host = host;
  context->producer = producer;
  state->channel = channel;
  state->producer = producer;
  state->worker_done = 0;
  if (!start_worker(context)) {
    state->channel = NULL;
    state->producer = NULL;
    mutex_unlock(&state->mutex);
    free(context);
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    host->set_error(call, TEXT("native.io"), TEXT("cannot start standard-input reader"));
    return SLUG_FFI_ERROR;
  }
  mutex_unlock(&state->mutex);

  return host->set_channel_clone(call, channel) ? SLUG_FFI_OK : SLUG_FFI_ERROR;
}

static void destroy_library(void *raw_state) {
  stdin_state *state = raw_state;
  slug_ffi_channel *channel;
  slug_ffi_producer *producer = NULL;
  int worker_done;

  if (state == NULL) return;
  mutex_lock(&state->mutex);
  state->shutdown_requested = 1;
  channel = state->channel;
  state->channel = NULL;
  if (state->producer != NULL) state->host->producer_close(state->producer);
  worker_done = state->worker_done;
  if (worker_done) {
    producer = state->producer;
    state->producer = NULL;
  }
  mutex_unlock(&state->mutex);

  if (channel != NULL) state->host->channel_destroy(channel);
  if (producer != NULL) {
    state->host->producer_destroy(producer);
    mutex_destroy(&state->mutex);
    free(state);
  }
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), TEXT("readLines"),
     TEXT("stdin.read_lines/v1"), 0, 0, read_lines},
};

static const slug_ffi_module_descriptor MODULE = {
    SLUG_FFI_PROTOTYPE_ABI_MAJOR,
    SLUG_FFI_PROTOTYPE_ABI_MINOR,
    sizeof(slug_ffi_module_descriptor),
    TEXT("slug.io.stdin"),
    FUNCTIONS,
    1,
    NULL,
    0,
};

static const slug_ffi_library_descriptor LIBRARY = {
    SLUG_FFI_PROTOTYPE_ABI_MAJOR,
    SLUG_FFI_PROTOTYPE_ABI_MINOR,
    sizeof(slug_ffi_library_descriptor),
    destroy_library,
    &MODULE,
    1,
};

SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(
    const slug_ffi_host_api *host, void **out_state) {
  stdin_state *state;
  if (host == NULL || out_state == NULL) return NULL;
  state = calloc(1, sizeof(stdin_state));
  if (state == NULL) return NULL;
  state->host = host;
  mutex_init(&state->mutex);
  *out_state = state;
  return &LIBRARY;
}
