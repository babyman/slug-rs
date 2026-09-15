#include "slug_ffi_prototype.h"
#include <stdlib.h>

static void destroy_resource(void *resource) {
  free(resource);
}

static int32_t create_counter(const slug_ffi_host_api *host, slug_ffi_call *call,
                              void *state) {
  (void)state;
  void *resource = malloc(1);
  if (resource == NULL) {
    host->set_error(call, (slug_ffi_text){"resource.alloc", 14},
                    (slug_ffi_text){"cannot allocate resource", 24});
    return SLUG_FFI_ERROR;
  }
  if (!host->set_resource(call, (slug_ffi_text){"slug.resource_result.Counter", 28}, resource)) {
    free(resource);
    return SLUG_FFI_ERROR;
  }
  return SLUG_FFI_OK;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"create", 6}, {"resource.create/v1", 18}, 0, 0, create_counter},
};

static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), {"Counter", 7}, destroy_resource},
  {sizeof(slug_ffi_resource_descriptor), {"Other", 5}, destroy_resource},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.resource_result", 20},
  FUNCTIONS,
  1,
  RESOURCES,
  2,
};

static const slug_ffi_library_descriptor LIBRARY = {SLUG_FFI_PROTOTYPE_ABI_MAJOR, SLUG_FFI_PROTOTYPE_ABI_MINOR, sizeof(slug_ffi_library_descriptor), NULL, &MODULE, 1};
const slug_ffi_library_descriptor *slug_ffi_library_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &LIBRARY;
}
