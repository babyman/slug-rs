#include "slug_ffi_prototype.h"
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <unistd.h>

typedef struct {
  const slug_ffi_host_api *host;
  slug_ffi_producer *producer;
} send_context;

static _Atomic int completed = 0;
static _Atomic int send_status = SLUG_FFI_PRODUCER_INVALID;

static void *send_after_receiver_drop(void *raw_context) {
  send_context *context = raw_context;
  usleep(10000);
  atomic_store(&send_status, context->host->producer_send_i64(context->producer, 7));
  context->host->producer_destroy(context->producer);
  atomic_store(&completed, 1);
  free(context);
  return NULL;
}

static int32_t delayed(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  send_context *context = malloc(sizeof(send_context));
  if (context == NULL) return SLUG_FFI_ERROR;
  atomic_store(&completed, 0);
  atomic_store(&send_status, SLUG_FFI_PRODUCER_INVALID);
  slug_ffi_producer *producer = NULL;
  slug_ffi_channel *channel = host->channel_create(call, 1, &producer);
  if (channel == NULL) {
    free(context);
    return SLUG_FFI_ERROR;
  }
  context->host = host;
  context->producer = producer;
  pthread_t thread;
  if (pthread_create(&thread, NULL, send_after_receiver_drop, context) != 0) {
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    free(context);
    return SLUG_FFI_ERROR;
  }
  pthread_detach(thread);
  return host->set_channel(call, channel) ? SLUG_FFI_OK : SLUG_FFI_ERROR;
}

static int32_t wait_status(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  while (!atomic_load(&completed)) usleep(1000);
  host->set_i64(call, atomic_load(&send_status));
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"delayed", 7}, {"revocation.delayed/v1", 21}, 0, 0, delayed},
  {sizeof(slug_ffi_function_descriptor), {"waitStatus", 10}, {"revocation.wait_status/v1", 25}, 0, 0, wait_status},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.revocation", 15},
  NULL,
  FUNCTIONS,
  2,
  NULL,
  0,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
