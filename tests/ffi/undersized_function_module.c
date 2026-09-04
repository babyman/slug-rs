#include "slug_ffi_prototype.h"

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {0, {"broken", 6}, {"broken/v1", 9}, 0, 0, NULL},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.broken", 11},
  FUNCTIONS,
  1,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host) {
  return host == NULL ? NULL : &MODULE;
}
