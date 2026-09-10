#include "slug_ffi_prototype.h"
#include <stdatomic.h>
#include <stdlib.h>

static _Atomic int accepted = 0;
static _Atomic int closed_status = SLUG_FFI_PRODUCER_INVALID;
static _Atomic int bytes_freed = 0;

static void destroy_bytes(void *bytes) {
  atomic_fetch_add(&bytes_freed, 1);
  free(bytes);
}

static int32_t values(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  atomic_store(&accepted, 0);
  atomic_store(&closed_status, SLUG_FFI_PRODUCER_INVALID);
  atomic_store(&bytes_freed, 0);

  slug_ffi_producer *producer = NULL;
  slug_ffi_channel *channel = host->channel_create(call, 4, &producer);
  char *bytes = malloc(3);
  if (channel == NULL || bytes == NULL) {
    if (channel != NULL) host->channel_destroy(channel);
    if (producer != NULL) host->producer_destroy(producer);
    free(bytes);
    return SLUG_FFI_ERROR;
  }
  bytes[0] = 1;
  bytes[1] = 2;
  bytes[2] = 3;

  int32_t statuses[] = {
      host->producer_send_nil(producer),
      host->producer_send_bool(producer, true),
      host->producer_send_f64(producer, 1.5),
      host->producer_send_bytes(producer, (slug_ffi_text){bytes, 3}, destroy_bytes),
  };
  for (size_t index = 0; index < sizeof(statuses) / sizeof(statuses[0]); index++) {
    if (statuses[index] == SLUG_FFI_PRODUCER_SENT) {
      atomic_fetch_add(&accepted, 1);
      continue;
    }
    if (index == 3) destroy_bytes(bytes);
    host->producer_close(producer);
    host->producer_destroy(producer);
    host->channel_destroy(channel);
    return SLUG_FFI_ERROR;
  }

  host->producer_close(producer);
  atomic_store(&closed_status, host->producer_send_i64(producer, 99));
  host->producer_destroy(producer);
  return host->set_channel(call, channel) ? SLUG_FFI_OK : SLUG_FFI_ERROR;
}

static int32_t accepted_count(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)state;
  host->set_i64(call, atomic_load(&accepted));
  return SLUG_FFI_OK;
}

static int32_t closed_status_value(const slug_ffi_host_api *host, slug_ffi_call *call,
                                   void *state) {
  (void)state;
  host->set_i64(call, atomic_load(&closed_status));
  return SLUG_FFI_OK;
}

static int32_t bytes_freed_count(const slug_ffi_host_api *host, slug_ffi_call *call,
                                 void *state) {
  (void)state;
  host->set_i64(call, atomic_load(&bytes_freed));
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
    {sizeof(slug_ffi_function_descriptor), {"values", 6}, {"producer.values/v1", 18}, 0, 0, values},
    {sizeof(slug_ffi_function_descriptor), {"accepted", 8}, {"producer.accepted/v1", 20}, 0, 0, accepted_count},
    {sizeof(slug_ffi_function_descriptor), {"closedStatus", 12}, {"producer.closed/v1", 18}, 0, 0, closed_status_value},
    {sizeof(slug_ffi_function_descriptor), {"bytesFreed", 10}, {"producer.bytes_freed/v1", 23}, 0, 0, bytes_freed_count},
};

static const slug_ffi_module_descriptor MODULE = {
    SLUG_FFI_PROTOTYPE_ABI_MAJOR,
    SLUG_FFI_PROTOTYPE_ABI_MINOR,
    sizeof(slug_ffi_module_descriptor),
    {"slug.producer", 13},
    FUNCTIONS,
    4,
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

const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host,
                                                         void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &LIBRARY;
}
