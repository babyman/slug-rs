#include "slug_ffi_prototype.h"

static int32_t unknown_status(const slug_ffi_host_api *host, slug_ffi_call *call, void *state) {
  (void)host;
  (void)call;
  (void)state;
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
  NULL,
  FUNCTIONS,
  1,
};

const slug_ffi_module_descriptor *slug_ffi_module_init(const slug_ffi_host_api *host,
                                                        void **module_state) {
  if (host == NULL || module_state == NULL) return NULL;
  *module_state = NULL;
  return &MODULE;
}
