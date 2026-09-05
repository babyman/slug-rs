#include "slug_ffi_prototype.h"
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct {
  const slug_ffi_host_api *host;
  slug_ffi_producer *producer;
} send_context;

static _Atomic int saw_full = 0;
static _Atomic int freed = 0;

static char *copy_text(const char *text) {
  size_t length = strlen(text);
  char *copy = malloc(length + 1);
  if (copy != NULL) memcpy(copy, text, length + 1);
  return copy;
}

static void destroy_text(void *raw_text) {
  atomic_fetch_add(&freed, 1);
  free(raw_text);
}

static int32_t send_text(const slug_ffi_host_api *host, slug_ffi_producer *producer,
                         char *text) {
  return host->producer_send_text(
      producer, (slug_ffi_text){text, strlen(text)}, destroy_text);
}

static void *send_values(void *raw_context) {
  send_context *context = raw_context;
  char *first = copy_text("first");
  char *second = copy_text("second");
  if (first == NULL || second == NULL) {
    if (first != NULL) destroy_text(first);
    if (second != NULL) destroy_text(second);
    context->host->producer_destroy(context->producer);
    free(context);
    return NULL;
  }
  int32_t status = send_text(context->host, context->producer, first);
  if (status != SLUG_FFI_PRODUCER_SENT) {
    destroy_text(first);
    destroy_text(second);
  } else {
    while ((status = send_text(context->host, context->producer, second)) ==
           SLUG_FFI_PRODUCER_FULL) {
      atomic_store(&saw_full, 1);
      usleep(1000);
    }
    if (status != SLUG_FFI_PRODUCER_SENT) destroy_text(second);
  }
  context->host->producer_destroy(context->producer);
  free(context);
  return NULL;
}

static int32_t backpressured_text(const slug_ffi_host_api *host, slug_ffi_call *call,
                                  void *state) {
  (void)state;
  send_context *context = malloc(sizeof(send_context));
  if (context == NULL) return SLUG_FFI_ERROR;
  atomic_store(&saw_full, 0);
  atomic_store(&freed, 0);
  slug_ffi_producer *producer = NULL;
  slug_ffi_channel *channel = host->channel_create(call, 1, &producer);
  if (channel == NULL) {
    free(context);
    return SLUG_FFI_ERROR;
  }
  context->host = host;
  context->producer = producer;
  pthread_t thread;
  if (pthread_create(&thread, NULL, send_values, context) != 0) {
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    free(context);
    return SLUG_FFI_ERROR;
  }
  pthread_detach(thread);
  return host->set_channel(call, channel) ? SLUG_FFI_OK : SLUG_FFI_ERROR;
}

static int32_t did_see_full(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  host->set_i64(call, atomic_load(&saw_full));
  return SLUG_FFI_OK;
}

static int32_t freed_count(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  host->set_i64(call, atomic_load(&freed));
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"backpressuredText", 17}, {"text.backpressure/v1", 20}, 0, 0, backpressured_text},
  {sizeof(slug_ffi_function_descriptor), {"sawFull", 7}, {"text.saw_full/v1", 16}, 0, 0, did_see_full},
  {sizeof(slug_ffi_function_descriptor), {"freed", 5}, {"text.freed/v1", 13}, 0, 0, freed_count},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.textbackpressure", 21},
  NULL,
  FUNCTIONS,
  3,
  NULL,
  0,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
