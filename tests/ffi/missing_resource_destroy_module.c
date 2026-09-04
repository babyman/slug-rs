#include "slug_ffi_prototype.h"

static const slug_ffi_resource_descriptor RESOURCES[] = {
  {sizeof(slug_ffi_resource_descriptor), {"counter", 7}, NULL},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.invalid_resource", 21},
  NULL,
  NULL,
  0,
  RESOURCES,
  1,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **out_state) {
  if (host == NULL || out_state == NULL) return NULL;
  *out_state = NULL;
  return &MODULE;
}
