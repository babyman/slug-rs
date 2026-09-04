#include "slug_ffi_prototype.h"
#include <stdlib.h>

typedef struct {
  int64_t value;
} counter_resource;

static int destroyed = 0;

static void destroy_counter(void *raw_resource) {
  destroyed += 1;
  free(raw_resource);
}

static int32_t create_counter(const slug_ffi_host_api *host, slug_ffi_call *call,
                              void *state) {
  (void)state;
  int64_t value = 0;
  if (!host->argument_i64(call, 0, &value)) return SLUG_FFI_ERROR;
  counter_resource *resource = malloc(sizeof(counter_resource));
  if (resource == NULL) {
    host->set_error(call, (slug_ffi_text){"resource.alloc", 14},
                    (slug_ffi_text){"cannot allocate counter", 23});
    return SLUG_FFI_ERROR;
  }
  resource->value = value;
  if (!host->set_resource(call, (slug_ffi_text){"counter", 7}, resource)) {
    free(resource);
    return SLUG_FFI_ERROR;
  }
  return SLUG_FFI_OK;
}

static int32_t read_counter(const slug_ffi_host_api *host, slug_ffi_call *call,
                            void *state) {
  (void)state;
  void *raw_resource = NULL;
  if (!host->argument_resource(call, 0, (slug_ffi_text){"counter", 7}, &raw_resource)) {
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, ((counter_resource *)raw_resource)->value);
  return SLUG_FFI_OK;
}

static int32_t close_counter(const slug_ffi_host_api *host, slug_ffi_call *call,
                             void *state) {
  (void)state;
  if (!host->close_resource(call, 0, (slug_ffi_text){"counter", 7})) {
    return SLUG_FFI_ERROR;
  }
  host->set_i64(call, destroyed);
  return SLUG_FFI_OK;
}

static int32_t destroyed_count(const slug_ffi_host_api *host, slug_ffi_call *call,
                               void *state) {
  (void)host;
  (void)state;
  host->set_i64(call, destroyed);
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"create", 6}, {"counter.create/v1", 17}, 1, 1, create_counter},
  {sizeof(slug_ffi_function_descriptor), {"read", 4}, {"counter.read/v1", 15}, 1, 1, read_counter},
  {sizeof(slug_ffi_function_descriptor), {"close", 5}, {"counter.close/v1", 16}, 1, 1, close_counter},
  {sizeof(slug_ffi_function_descriptor), {"destroyed", 9}, {"counter.destroyed/v1", 20}, 0, 0, destroyed_count},
};

static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), {"counter", 7}, destroy_counter},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.resources", 14},
  NULL,
  FUNCTIONS,
  4,
  RESOURCES,
  1,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
