#include "slug_ffi_prototype.h"

static int32_t unknown_status(const slug_ffi_host_api *host, slug_ffi_call *call) {
  (void)host;
  (void)call;
  return 99;
}

static const slug_ffi_function_descriptor FUNCTIONS[] = {
  {sizeof(slug_ffi_function_descriptor), {"status", 6}, {"status/v1", 9}, 0, 0, unknown_status},
};

static const slug_ffi_module_descriptor MODULE = {
  SLUG_FFI_PROTOTYPE_ABI_MAJOR,
  SLUG_FFI_PROTOTYPE_ABI_MINOR,
  sizeof(slug_ffi_module_descriptor),
  {"slug.status", 11},
  FUNCTIONS,
  1,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host) {
  return host == NULL ? NULL : &MODULE;
}
