#include "slug_ffi_prototype.h"
#include <pthread.h>
#include <stdlib.h>

typedef struct {
  const slug_ffi_host_api *host;
  slug_ffi_producer *producer;
} send_context;

static void *send_value(void *raw_context) {
  send_context *context = raw_context;
  (void)context->host->producer_send_i64(context->producer, 73);
  context->host->producer_destroy(context->producer);
  free(context);
  return NULL;
}

static int32_t delayed(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  send_context *context = malloc(sizeof(send_context));
  if (context == NULL) {
    host->set_error(call, (slug_ffi_text){"async.alloc", 11},
                    (slug_ffi_text){"cannot allocate send context", 28});
    return SLUG_FFI_ERROR;
  }
  slug_ffi_producer *producer = NULL;
  slug_ffi_channel *channel = host->channel_create(call, 1, &producer);
  if (channel == NULL) {
    free(context);
    return SLUG_FFI_ERROR;
  }
  context->host = host;
  context->producer = producer;
  pthread_t thread;
  if (pthread_create(&thread, NULL, send_value, context) != 0) {
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    free(context);
    host->set_error(call, (slug_ffi_text){"async.thread", 12},
                    (slug_ffi_text){"cannot start send thread", 24});
    return SLUG_FFI_ERROR;
  }
  pthread_detach(thread);
  if (!host->set_channel(call, channel)) {
    return SLUG_FFI_ERROR;
  }
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"delayed", 7}, {"async.delayed/v1", 16}, 0, 0, delayed},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.async", 10},
  NULL,
  FUNCTIONS,
  1,
  NULL,
  0,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
