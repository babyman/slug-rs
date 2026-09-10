#include "slug_ffi_prototype.h"

#include <stdio.h>
#include <stdlib.h>

#if defined(_WIN32)
#include <windows.h>
#else
#define _POSIX_C_SOURCE 200809L
#include <pthread.h>
#include <time.h>
#endif

#define TEXT(value) ((slug_ffi_text){value, sizeof(value) - 1})
#define STDIN_CHANNEL_CAPACITY 32u

typedef struct {
  const slug_ffi_host_api *host;
  slug_ffi_producer *producer;
} stdin_context;

static void destroy_text(void *text) {
  free(text);
}

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
        host->producer_destroy(producer);
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
        host->producer_destroy(producer);
        return;
      }
      char *expanded = realloc(line, next_capacity);
      if (expanded == NULL) {
        free(line);
        host->producer_close(producer);
        host->producer_destroy(producer);
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
    host->producer_destroy(producer);
    return;
  }
  if (line != NULL && !send_line(host, producer, line, length)) {
    host->producer_close(producer);
    host->producer_destroy(producer);
    return;
  }
  host->producer_close(producer);
  host->producer_destroy(producer);
}

#if defined(_WIN32)
static DWORD WINAPI stdin_worker(void *raw_context) {
  stdin_context *context = raw_context;
  read_stdin_lines(context->host, context->producer);
  free(context);
  return 0;
}
#else
static void *stdin_worker(void *raw_context) {
  stdin_context *context = raw_context;
  read_stdin_lines(context->host, context->producer);
  free(context);
  return NULL;
}
#endif

static int32_t open_lines(const slug_ffi_host_api *host, slug_ffi_call *call,
                          void *state) {
  (void)state;
  slug_ffi_producer *producer = NULL;
  slug_ffi_channel *channel =
      host->channel_create(call, STDIN_CHANNEL_CAPACITY, &producer);
  stdin_context *context = malloc(sizeof(stdin_context));
  if (channel == NULL || producer == NULL || context == NULL) {
    if (channel != NULL) host->channel_destroy(channel);
    if (producer != NULL) host->producer_destroy(producer);
    free(context);
    host->set_error(call, TEXT("native.io"), TEXT("cannot create standard-input stream"));
    return SLUG_FFI_ERROR;
  }
  context->host = host;
  context->producer = producer;

#if defined(_WIN32)
  HANDLE thread = CreateThread(NULL, 0, stdin_worker, context, 0, NULL);
  if (thread == NULL) {
    free(context);
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    host->set_error(call, TEXT("native.io"), TEXT("cannot start standard-input reader"));
    return SLUG_FFI_ERROR;
  }
  CloseHandle(thread);
#else
  pthread_t thread;
  if (pthread_create(&thread, NULL, stdin_worker, context) != 0) {
    free(context);
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    host->set_error(call, TEXT("native.io"), TEXT("cannot start standard-input reader"));
    return SLUG_FFI_ERROR;
  }
  pthread_detach(thread);
#endif

  if (host->set_channel(call, channel)) return SLUG_FFI_OK;
  host->channel_destroy(channel);
  return SLUG_FFI_ERROR;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), TEXT("openLines"),
     TEXT("stdin.open_lines/v1"), 0, 0, open_lines},
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
    NULL,
    &MODULE,
    1,
};

SLUG_FFI_PROTOTYPE_EXPORT const slug_ffi_library_descriptor *slug_ffi_library_init(
    const slug_ffi_host_api *host, void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &LIBRARY;
}
